//! Opening and unpacking a .purr archive.
//!
//! Unpacking goes pack by pack, in file order, so each pack is decoded once and memory stays
//! small. A chunk is written to every place that uses it (that is how repeats come back), and
//! every chunk is checked against its stored check before it is written.

use super::pool::{Deliver, Pool};
use super::seal::{self, Key};
use super::wire::*;
use super::{codec, jpeg, recovery};
use crate::safe::Sink;
use crate::{Entry, Error, Result, Wanted};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub struct Archive {
    path: PathBuf,
    pub header: Header,
    pub index: Index,
    key: Option<Key>,
    pub has_repair_data: bool,
}

fn damaged(what: &str) -> Error {
    Error::Damaged(what.to_string())
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn open(path: &Path, password: Option<&str>) -> Result<Archive> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut hb = [0u8; HEADER_LEN];
    if len < (HEADER_LEN + FOOTER_LEN) as u64 || f.read_exact(&mut hb).is_err() {
        return Err(damaged("the archive is cut short"));
    }
    let header = Header::parse(&hb)?;
    let key = match &header.kdf {
        Some(kdf) => {
            let pw = password.filter(|p| !p.is_empty()).ok_or(Error::NeedPassword)?;
            let key = seal::derive(pw, kdf)?;
            if !same(&key.header_check(Header::check_prefix(&hb)), &header.key_check) {
                return Err(Error::WrongPassword);
            }
            Some(key)
        }
        None => None,
    };
    // With repair data, the archive body ends before it.
    let geo = recovery::locate_tail(&mut f);
    let body_len = geo.as_ref().map(|g| g.body_len).unwrap_or(len);
    let mut fb = [0u8; FOOTER_LEN];
    if body_len < (HEADER_LEN + FOOTER_LEN) as u64 || f.seek(SeekFrom::Start(body_len - FOOTER_LEN as u64)).is_err() || f.read_exact(&mut fb).is_err() {
        return Err(damaged("the archive's end is missing"));
    }
    let footer = Footer::parse(&fb).ok_or_else(|| damaged("the archive's end is damaged or missing"))?;
    let index_end = footer.index_offset.checked_add(footer.index_stored);
    if footer.index_offset < HEADER_LEN as u64 || index_end != Some(body_len - FOOTER_LEN as u64) || footer.index_raw > MAX_INDEX_RAW {
        return Err(damaged("the archive's end doesn't add up"));
    }
    if footer.index_codec != CODEC_ZSTD {
        return Err(Error::Unsupported("a .purr made by a newer Purrzip. Update Purrzip to open it".into()));
    }
    f.seek(SeekFrom::Start(footer.index_offset))?;
    let stored = BufReader::with_capacity(128 * 1024, (&mut f).take(footer.index_stored));
    let plain: Box<dyn Read + '_> = match &key {
        Some(k) => Box::new(k.unseal(stored, footer.index_stored)),
        None => Box::new(stored),
    };
    let mut raw = Vec::with_capacity(footer.index_raw.min(64 << 20) as usize);
    codec::decoder(CODEC_ZSTD, plain)?
        .take(footer.index_raw + 1)
        .read_to_end(&mut raw)
        .map_err(|_| damaged("the archive's table of contents is damaged"))?;
    if raw.len() as u64 != footer.index_raw {
        return Err(damaged("the archive's table of contents is damaged"));
    }
    if key.is_none() && !same(&index_hash(&raw), &footer.index_hash) {
        return Err(damaged("the archive's table of contents is damaged"));
    }
    let index = Index::parse(&raw, footer.index_offset)?;
    Ok(Archive { path: path.to_path_buf(), header, index, key, has_repair_data: geo.is_some() })
}

struct Slot {
    abs: PathBuf,
    rel: String,
    name: String,
    index: usize,
    size: u64,
    left: u64,
    created: bool,
    finished: bool,
    mtime_ms: i64,
    mode: u32,
}

/// The files being written. Keeps a few handles open; a file is finished (time stamp set,
/// reported) the moment its last byte lands.
#[derive(Default)]
struct Outputs {
    slots: Vec<Slot>,
    open: Vec<(u32, File)>,
}

const MAX_OPEN: usize = 32;

impl Outputs {
    fn handle(&mut self, slot: u32) -> Result<&mut File> {
        if let Some(pos) = self.open.iter().position(|(s, _)| *s == slot) {
            let h = self.open.remove(pos);
            self.open.push(h);
        } else {
            let s = &mut self.slots[slot as usize];
            let f = if s.created { OpenOptions::new().write(true).open(&s.abs)? } else { OpenOptions::new().write(true).create_new(true).open(&s.abs)? };
            s.created = true;
            if self.open.len() >= MAX_OPEN {
                self.open.remove(0);
            }
            self.open.push((slot, f));
        }
        Ok(&mut self.open.last_mut().unwrap().1)
    }

    fn write_at(&mut self, slot: u32, offset: u64, data: &[u8], sink: &mut Sink) -> Result<()> {
        {
            let s = &self.slots[slot as usize];
            if s.finished || offset.checked_add(data.len() as u64).map(|e| e > s.size).unwrap_or(true) {
                return Err(damaged("the archive's table of contents doesn't add up"));
            }
        }
        if !data.is_empty() {
            let f = self.handle(slot)?;
            f.seek(SeekFrom::Start(offset))?;
            f.write_all(data)?;
        }
        let s = &mut self.slots[slot as usize];
        s.left = s.left.saturating_sub(data.len() as u64);
        sink.progress(data.len() as u64, &s.name);
        if s.left == 0 {
            self.finish(slot, sink)?;
        }
        Ok(())
    }

    fn finish(&mut self, slot: u32, sink: &mut Sink) -> Result<()> {
        self.handle(slot)?; // creates empty files, and brings the handle to the end of the list
        let (_, f) = self.open.pop().unwrap();
        let s = &mut self.slots[slot as usize];
        crate::safe::set_mtime(&f, s.mtime_ms.div_euclid(1000));
        #[cfg(unix)]
        if s.mode & 0o111 != 0 {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(m) = f.metadata() {
                let mut p = m.permissions();
                // executable for whoever may read it
                p.set_mode(p.mode() | ((p.mode() & 0o444) >> 2));
                let _ = f.set_permissions(p);
            }
        }
        drop(f);
        s.finished = true;
        sink.mark_written(s.index, s.rel.clone(), s.size);
        Ok(())
    }

    /// After an error or a Stop: remove files that were started but not completed.
    fn remove_unfinished(&mut self) {
        self.open.clear();
        for s in &self.slots {
            if s.created && !s.finished {
                let _ = std::fs::remove_file(&s.abs);
            }
        }
    }
}

struct PhotoJob {
    blob: Vec<u8>,
    size: u64,
    orig_check: u64,
    slots: Vec<u32>,
}
struct PhotoOut {
    slots: Vec<u32>,
    data: Result<Vec<u8>>,
}

struct State<'s, 'p> {
    out: Outputs,
    sink: &'s mut Sink<'p>,
}
impl Deliver<PhotoOut, Error> for State<'_, '_> {
    fn deliver(&mut self, o: PhotoOut) -> Result<()> {
        let data = o.data?;
        for slot in o.slots {
            self.out.write_at(slot, 0, &data, self.sink)?;
        }
        Ok(())
    }
    fn idle(&mut self) -> Result<()> {
        self.sink.check()
    }
}

struct Blob {
    size: u64,
    orig_check: u64,
    slots: Vec<u32>,
    parts: Vec<Option<Vec<u8>>>,
    missing: usize,
}

impl Archive {
    pub fn encrypted(&self) -> bool {
        self.header.encrypted()
    }

    pub fn entries(&self) -> Vec<Entry> {
        let locked = self.encrypted();
        self.index
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| Entry { index: i, path: f.path.trim_end_matches('/').to_string(), size: f.size, packed: None, is_dir: f.is_dir, encrypted: locked, modified: Some(f.mtime_ms.div_euclid(1000)), special: false })
            .collect()
    }

    fn pack_reader<'a>(&'a self, f: &'a mut File, p: &PackInfo) -> Result<Box<dyn Read + 'a>> {
        f.seek(SeekFrom::Start(p.offset))?;
        let stored = BufReader::with_capacity(128 * 1024, f.take(p.stored_len));
        let plain: Box<dyn Read + 'a> = match &self.key {
            Some(k) => Box::new(k.unseal(stored, p.stored_len)),
            None => Box::new(stored),
        };
        codec::decoder(p.codec, plain)
    }

    pub fn extract(&self, wanted: &Wanted, sink: &mut Sink) -> Result<()> {
        let mut state = State { out: Outputs::default(), sink };
        let r = self.extract_inner(wanted, &mut state);
        if r.is_err() {
            state.out.remove_unfinished();
        }
        r
    }

    fn extract_inner(&self, wanted: &Wanted, state: &mut State<'_, '_>) -> Result<()> {
        let ix = &self.index;
        // 1. Plan: where does every needed chunk go?
        let mut refs: Vec<(u32, u32, u64)> = Vec::new(); // (chunk, output slot, offset in the file)
        let mut blobs: Vec<Blob> = Vec::new();
        let mut blob_by_ext: HashMap<&[(u32, u32)], usize> = HashMap::new();
        let mut blob_refs: Vec<(u32, u32, u32)> = Vec::new(); // (chunk, blob, part)
        for (i, f) in ix.files.iter().enumerate() {
            if !wanted.has(i) {
                continue;
            }
            state.sink.check()?;
            if f.is_dir {
                state.sink.dir(i, &f.path)?;
                continue;
            }
            let Some((abs, rel)) = state.sink.reserve(&f.path)? else { continue };
            let slot = state.out.slots.len() as u32;
            state.out.slots.push(Slot { abs, rel, name: f.path.clone(), index: i, size: f.size, left: f.size, created: false, finished: false, mtime_ms: f.mtime_ms, mode: f.mode });
            let ext = &ix.extents[f.ext_start as usize..(f.ext_start + f.ext_count) as usize];
            if f.transform == TRANSFORM_NONE {
                let mut pos = 0u64;
                for &(first, count) in ext {
                    for c in first..first + count {
                        refs.push((c, slot, pos));
                        pos += ix.chunks[c as usize].len;
                    }
                }
            } else {
                let b = match blob_by_ext.get(ext) {
                    Some(&b) => b,
                    None => {
                        let b = blobs.len();
                        let mut parts = 0u32;
                        for &(first, count) in ext {
                            for c in first..first + count {
                                blob_refs.push((c, b as u32, parts));
                                parts += 1;
                            }
                        }
                        blobs.push(Blob { size: f.size, orig_check: f.orig_check, slots: Vec::new(), parts: (0..parts).map(|_| None).collect(), missing: parts as usize });
                        blob_by_ext.insert(ext, b);
                        b
                    }
                };
                if blobs[b].size != f.size || blobs[b].orig_check != f.orig_check {
                    return Err(damaged("the archive's table of contents doesn't add up"));
                }
                blobs[b].slots.push(slot);
            }
        }
        refs.sort_unstable();
        blob_refs.sort_unstable();

        // 2. Decode the packs that hold something we need, once each, in order.
        // Restoring photos is the slow part of unpacking: use the cores, but go easy on phones.
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(if cfg!(target_os = "android") { 3 } else { 8 });
        let restore = |job: PhotoJob| -> PhotoOut {
            let data = jpeg::restore(&job.blob, job.size).and_then(|jpg| {
                let h = blake3::hash(&jpg);
                if u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap()) == job.orig_check { Ok(jpg) } else { Err(damaged("a picture didn't come back the way it went in")) }
            });
            PhotoOut { slots: job.slots, data }
        };
        std::thread::scope(|scope| -> Result<()> {
            let mut pool: Pool<PhotoJob, PhotoOut> = Pool::new(scope, if blobs.is_empty() { 1 } else { workers }, workers * 2, &restore);
            let mut f = File::open(&self.path)?;
            let mut fd = File::open(&self.path)?; // second handle for reading single pieces directly
            let (mut ri, mut bi) = (0usize, 0usize);
            let mut buf: Vec<u8> = Vec::new();
            while ri < refs.len() || bi < blob_refs.len() {
                let next = match (refs.get(ri), blob_refs.get(bi)) {
                    (Some(a), Some(b)) => a.0.min(b.0),
                    (Some(a), None) => a.0,
                    (None, Some(b)) => b.0,
                    (None, None) => unreachable!(),
                };
                let p = &ix.packs[ix.chunks[next as usize].pack as usize];
                let pack_end = p.first_chunk + p.chunk_count;
                // the last chunk of this pack that anything needs
                let n_refs = refs[ri..].partition_point(|r| r.0 < pack_end);
                let n_blob = blob_refs[bi..].partition_point(|r| r.0 < pack_end);
                let last = match (n_refs, n_blob) {
                    (0, _) => blob_refs[bi + n_blob - 1].0,
                    (_, 0) => refs[ri + n_refs - 1].0,
                    _ => refs[ri + n_refs - 1].0.max(blob_refs[bi + n_blob - 1].0),
                };
                // Plain stored packs can be read piece by piece; everything else is decoded in order.
                let direct = p.codec == CODEC_STORE && self.key.is_none();
                let mut rd: Option<Box<dyn Read + '_>> = if direct { None } else { Some(self.pack_reader(&mut f, p)?) };
                let mut c = if direct { next } else { p.first_chunk };
                while c <= last {
                    let ch = &ix.chunks[c as usize];
                    let need_plain = ri < refs.len() && refs[ri].0 == c;
                    let need_blob = bi < blob_refs.len() && blob_refs[bi].0 == c;
                    if direct && !need_plain && !need_blob {
                        c += 1;
                        continue;
                    }
                    buf.resize(ch.len as usize, 0);
                    let got = match rd.as_mut() {
                        Some(r) => r.read_exact(&mut buf),
                        None => fd.seek(SeekFrom::Start(p.offset + ch.off)).and_then(|_| fd.read_exact(&mut buf)),
                    };
                    got.map_err(|_| damaged("part of the archive is damaged"))?;
                    if need_plain || need_blob {
                        let h = blake3::hash(&buf);
                        if check_of(h.as_bytes(), ch.len) != ch.check {
                            return Err(damaged("part of the archive is damaged"));
                        }
                    }
                    while ri < refs.len() && refs[ri].0 == c {
                        state.out.write_at(refs[ri].1, refs[ri].2, &buf, state.sink)?;
                        ri += 1;
                    }
                    while bi < blob_refs.len() && blob_refs[bi].0 == c {
                        let (_, b, part) = blob_refs[bi];
                        bi += 1;
                        let blob = &mut blobs[b as usize];
                        if blob.parts[part as usize].is_none() {
                            blob.missing -= 1;
                        }
                        blob.parts[part as usize] = Some(buf.clone());
                        if blob.missing == 0 {
                            let parts = std::mem::take(&mut blob.parts);
                            let bytes = if parts.len() == 1 { parts.into_iter().next().unwrap().unwrap() } else { parts.into_iter().flatten().flatten().collect() };
                            pool.submit(PhotoJob { blob: bytes, size: blob.size, orig_check: blob.orig_check, slots: std::mem::take(&mut blob.slots) }, state)?;
                        }
                    }
                    state.sink.check()?;
                    c += 1;
                }
            }
            pool.drain(state)
        })?;

        // 3. Empty files have no chunks: create them now.
        for slot in 0..state.out.slots.len() as u32 {
            let s = &state.out.slots[slot as usize];
            if !s.finished {
                if s.size != 0 {
                    return Err(damaged("a file is incomplete"));
                }
                state.out.finish(slot, state.sink)?;
            }
        }
        Ok(())
    }
}
