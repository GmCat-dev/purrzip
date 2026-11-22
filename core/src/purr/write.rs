//! Writing a .purr archive.
//!
//! 1. Look at every file's first bytes and sort the files into kinds (text, programs, other
//!    data, photos, already-packed).
//! 2. Kind by kind, cut files into chunks; a chunk seen before is only pointed at (that's the
//!    "store everything once" part). New chunks fill a pack.
//! 3. Full packs are squeezed on worker threads with the method that suits the kind, and written
//!    in order. Photos are shrunk on worker threads, each one checked to come back
//!    byte-identical. Already-packed data and shrunk photos get a fast pass that finds long
//!    repeats; if that gains nothing they are stored as they are.
//! 4. Last come the table of contents and the footer. Repair data is added afterwards.

use super::chunk::Chunker;
use super::codec::{self, Kind, Recipe};
use super::pool::{Deliver, Pool};
use super::seal;
use super::wire::*;
use super::{jpeg, PurrOptions};
use crate::create::Item;
use crate::{Error, Level, Progress, Result};
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

pub(crate) struct Tuning {
    pub pack_bytes: usize,
    /// Pack size for photos and already-packed files. These only get the fast long-range pass,
    /// which costs little memory, and a bigger pack lets it see repeats that are further apart
    /// (measured: a folder holding the same pictures twice with different labels went from
    /// 187 MB to under 160 MB).
    pub store_pack_bytes: usize,
    pub pack_workers: usize,
    pub photo_workers: usize,
    pub photos: bool,
    pub max_pixels: u64,
}

pub(crate) fn tuning(level: Level, low: bool) -> Tuning {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    let photo_workers = cores.min(if low { 3 } else { 8 });
    let max_pixels = if low { jpeg::MAX_JPEG_PIXELS_LOW_MEM } else { jpeg::MAX_JPEG_PIXELS };
    let store_pack_bytes = (if low { 32 } else { 128 }) << 20;
    match level {
        Level::Store | Level::Fast => Tuning { pack_bytes: (if low { 8 } else { 16 }) << 20, store_pack_bytes, pack_workers: cores.min(if low { 2 } else { 4 }), photo_workers: 1, photos: false, max_pixels: 0 },
        Level::Normal => Tuning {
            pack_bytes: (if low { 16 } else { 32 }) << 20,
            store_pack_bytes,
            pack_workers: if low { cores.min(2) } else { cores.saturating_sub(1).clamp(1, 3) },
            photo_workers,
            photos: true,
            max_pixels,
        },
        // The strongest settings need the most memory (about 0.9 GB for one worker), so one worker:
        // slower, but safe on an 8 GB laptop.
        // Packs are twice the 64 MiB dictionary: measured, a folder of programs came out 2.4% bigger
        // with 64 MiB packs, because every new pack starts with an empty dictionary.
        Level::Max => Tuning { pack_bytes: (if low { 32 } else { 128 }) << 20, store_pack_bytes, pack_workers: 1, photo_workers, photos: true, max_pixels },
    }
}

/// Which methods to try for a pack of this kind. More than one means "try each, keep the smallest".
pub(crate) fn recipes(level: Level, low: bool, kind: Kind) -> Vec<Recipe> {
    let bcj = match kind {
        Kind::Exec(t) => Some(codec::bcj_of(t)),
        _ => None,
    };
    if level != Level::Store && matches!(kind, Kind::Packed | Kind::Photo) {
        // Already-packed files and shrunk photos don't squeeze, but they often share long identical
        // stretches (versions of the same zip, jar or document; the same picture saved with
        // different labels). A fast long-range pass finds those; when it gains nothing the pack
        // is stored as it is.
        return vec![Recipe::Zstd { level: 1, long: true }];
    }
    match level {
        Level::Store => vec![Recipe::Store],
        Level::Fast => vec![Recipe::Zstd { level: 3, long: false }],
        Level::Normal => vec![Recipe::Xz { preset: 6, dict: (if low { 8 } else { 16 }) << 20, extreme: false, bcj }],
        // Measured on real folders (2026-11): LZMA2 with a 64 MiB dictionary beat Zstandard 22 on
        // every kind. PPMd won on some text by 2-3% but lost on other text, unpacks ten times
        // slower and needs ~190 MiB to unpack, so the writer doesn't use it (readers support it).
        Level::Max => vec![Recipe::Xz { preset: 9, dict: (if low { 16 } else { 64 }) << 20, extreme: true, bcj }],
    }
}
/// If a slow-to-unpack method (PPMd) is ever tried, it must win by this much to be picked.
const PPMD_MUST_WIN_BY: f64 = 0.02;

struct ChunkMeta {
    len: u64,
    check: u64,
}

struct PackOut {
    codec: u8,
    stored: Vec<u8>,
    raw_len: u64,
    first_chunk: u32,
    chunk_count: u32,
}

enum Job {
    Pack { raw: Vec<u8>, recipes: Vec<Recipe>, first_chunk: u32, chunk_count: u32, cell: Arc<AtomicU64> },
    Photo { data: Vec<u8> },
}
enum Out {
    Pack(Result<PackOut>),
    /// A photo shrunk to a Lepton stream.
    Shrunk(Vec<u8>),
    /// A photo that is stored unchanged (here are its bytes back).
    Keep(Vec<u8>),
}

/// Try each method on a pack and keep the best. Runs on a worker thread.
fn squeeze(raw: &[u8], recipes: &[Recipe], cell: &AtomicU64, stop: &AtomicBool) -> Result<(u8, Vec<u8>)> {
    let n = recipes.len().max(1);
    let mut best: Option<(Recipe, Vec<u8>)> = None;
    for (i, r) in recipes.iter().enumerate() {
        if matches!(r, Recipe::Store) {
            continue;
        }
        let out = codec::compress_ticking(r, raw, &mut |done| {
            cell.store(((i * raw.len() + done) / n) as u64, Ordering::Relaxed);
            !stop.load(Ordering::Relaxed)
        })?;
        let better = match &best {
            None => true,
            Some((br, b)) => {
                let (new_ppmd, old_ppmd) = (matches!(r, Recipe::Ppmd { .. }), matches!(br, Recipe::Ppmd { .. }));
                if new_ppmd && !old_ppmd {
                    (out.len() as f64) < b.len() as f64 * (1.0 - PPMD_MUST_WIN_BY)
                } else if old_ppmd && !new_ppmd {
                    (out.len() as f64) * (1.0 - PPMD_MUST_WIN_BY) <= b.len() as f64
                } else {
                    out.len() < b.len()
                }
            }
        };
        if better {
            best = Some((*r, out));
        }
    }
    cell.store(raw.len() as u64, Ordering::Relaxed);
    match best {
        // Not worth it if squeezing saves under 2%: store as it is (unpacks instantly).
        Some((r, out)) if (out.len() as f64) < raw.len() as f64 * 0.98 => Ok((r.codec(), out)),
        _ => Ok((CODEC_STORE, raw.to_vec())),
    }
}

struct Meta {
    mtime_ms: i64,
    mode: u32,
}

fn meta_of(path: &Path) -> Meta {
    let m = fs::metadata(path).ok();
    let mtime_ms = m
        .as_ref()
        .and_then(|m| m.modified().ok())
        .map(|t| match t.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_millis() as i64,
            Err(e) => -(e.duration().as_millis() as i64),
        })
        .unwrap_or(0);
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        m.as_ref().map(|m| m.permissions().mode() & 0o777).unwrap_or(0)
    };
    #[cfg(not(unix))]
    let mode = 0;
    Meta { mtime_ms, mode }
}

enum PhotoState {
    /// Being shrunk right now; these other files are the same picture.
    Queued(Vec<usize>),
    Blob { chunk: u32, size: u64, orig_check: u64 },
    Raw { ext: Vec<(u32, u32)>, size: u64 },
}

struct W<'a> {
    out: BufWriter<File>,
    offset: u64,
    progress: &'a dyn Progress,
    stop: &'a AtomicBool,
    total: u64,
    pack_bytes: usize,
    packs: Vec<PackInfo>,
    chunks: Vec<ChunkMeta>,
    seen: HashMap<u128, u32>,
    cur: Vec<u8>,
    cur_first: u32,
    items: &'a [Item],
    metas: Vec<Meta>,
    files: Vec<Option<FileInfo>>,
    extents: Vec<(u32, u32)>,
    stats: Stats,
    read_done: u64,
    in_pool: u64,
    cells: VecDeque<(u64, Arc<AtomicU64>)>,
    last_report: u64,
    current: String,
    photo_queue: VecDeque<(u128, usize, u64, u64)>,
    photos: HashMap<u128, PhotoState>,
    chunker: Chunker,
    /// Methods for the pack being filled, and full packs waiting to be handed to the workers.
    cur_recipes: Vec<Recipe>,
    pending: VecDeque<Job>,
}

fn key128(h: &blake3::Hash) -> u128 {
    u128::from_le_bytes(h.as_bytes()[..16].try_into().unwrap())
}

fn push_extent(ext: &mut Vec<(u32, u32)>, id: u32) {
    match ext.last_mut() {
        Some((first, count)) if first.checked_add(*count) == Some(id) => *count += 1,
        _ => ext.push((id, 1)),
    }
}

impl<'a> W<'a> {
    /// Add a chunk: returns its id, and whether it was new (false = a repeat of stored data).
    fn add_chunk(&mut self, data: &[u8]) -> Result<(u32, bool)> {
        let h = blake3::hash(data);
        let k = key128(&h);
        if let Some(&id) = self.seen.get(&k) {
            return Ok((id, false));
        }
        if self.chunks.len() >= u32::MAX as usize - 1 {
            return Err(Error::Other("Too many pieces for one archive.".into()));
        }
        let id = self.chunks.len() as u32;
        self.chunks.push(ChunkMeta { len: data.len() as u64, check: check_of(h.as_bytes(), data.len() as u64) });
        self.cur.extend_from_slice(data);
        self.seen.insert(k, id);
        Ok((id, true))
    }

    fn pack_full(&self) -> bool {
        self.cur.len() >= self.pack_bytes
    }

    /// Close the open pack and queue it for the workers (the main loop hands queued packs over).
    fn roll_pack(&mut self) {
        if self.cur.is_empty() {
            return;
        }
        let raw = std::mem::replace(&mut self.cur, Vec::with_capacity(self.pack_bytes.min(8 << 20)));
        let first_chunk = self.cur_first;
        let chunk_count = self.chunks.len() as u32 - first_chunk;
        self.cur_first = self.chunks.len() as u32;
        let cell = Arc::new(AtomicU64::new(0));
        self.in_pool += raw.len() as u64;
        self.cells.push_back((raw.len() as u64, cell.clone()));
        self.pending.push_back(Job::Pack { raw, recipes: self.cur_recipes.clone(), first_chunk, chunk_count, cell });
    }

    fn write_pack(&mut self, p: PackOut) -> Result<()> {
        self.out.write_all(&p.stored)?;
        self.packs.push(PackInfo { offset: self.offset, stored_len: p.stored.len() as u64, raw_len: p.raw_len, codec: p.codec, first_chunk: p.first_chunk, chunk_count: p.chunk_count });
        self.offset += p.stored.len() as u64;
        Ok(())
    }

    /// Report progress and notice a Stop.
    fn tick(&mut self, force: bool) -> Result<()> {
        if self.progress.cancelled() {
            self.stop.store(true, Ordering::Relaxed);
            return Err(Error::Cancelled);
        }
        // Done = read so far, minus what is still waiting (in the open pack or with the workers),
        // plus how far the workers have got. It never goes backwards.
        let working: u64 = self.cells.iter().map(|(len, c)| c.load(Ordering::Relaxed).min(*len)).sum();
        let done = (self.read_done + working).saturating_sub(self.in_pool + self.cur.len() as u64).min(self.total).max(self.last_report);
        if force || done - self.last_report >= 256 * 1024 {
            self.last_report = done;
            self.progress.update(done, self.total, &self.current);
        }
        Ok(())
    }

    fn finish_file(&mut self, item: usize, size: u64, transform: u8, orig_check: u64, ext: &[(u32, u32)]) {
        let ext_start = self.extents.len() as u32;
        self.extents.extend_from_slice(ext);
        let m = &self.metas[item];
        self.files[item] = Some(FileInfo {
            path: self.items[item].name.clone(),
            is_dir: false,
            size,
            mtime_ms: m.mtime_ms,
            mode: m.mode,
            transform,
            orig_check,
            ext_start,
            ext_count: ext.len() as u32,
        });
        self.stats.total_in += size;
    }

    /// Chunk bytes held in memory into the open pack. Used for photos kept unchanged.
    fn add_bytes(&mut self, data: &[u8]) -> Result<Vec<(u32, u32)>> {
        let mut ext = Vec::new();
        let mut chunker = std::mem::take(&mut self.chunker);
        let r = chunker.run(&mut &data[..], &mut |ch| {
            let (id, new) = self.add_chunk(ch)?;
            if !new {
                self.stats.dup_bytes += ch.len() as u64;
            }
            push_extent(&mut ext, id);
            if self.pack_full() {
                self.roll_pack();
            }
            Ok(())
        });
        self.chunker = chunker;
        r?;
        Ok(ext)
    }

    fn photo_done(&mut self, shrunk: Option<Vec<u8>>, kept: Option<Vec<u8>>) -> Result<()> {
        let (k, first_item, size, orig_check) = self.photo_queue.pop_front().expect("photo result without a queued photo");
        let state = match (shrunk, kept) {
            (Some(blob), _) => {
                let (chunk, new) = self.add_chunk(&blob)?;
                if new {
                    self.stats.jpeg_in += size;
                    self.stats.jpeg_out += blob.len() as u64;
                    self.stats.jpeg_count += 1;
                } else {
                    self.stats.dup_bytes += size;
                }
                if self.pack_full() {
                    self.roll_pack();
                }
                PhotoState::Blob { chunk, size, orig_check }
            }
            (None, Some(data)) => PhotoState::Raw { ext: self.add_bytes(&data)?, size },
            (None, None) => unreachable!(),
        };
        let twins = match self.photos.insert(k, state) {
            Some(PhotoState::Queued(v)) => v,
            _ => Vec::new(),
        };
        self.photo_file(first_item, k, false);
        for t in twins {
            self.photo_file(t, k, true);
        }
        self.tick(false)
    }

    /// Record a photo file whose picture is already in the archive.
    fn photo_file(&mut self, item: usize, k: u128, repeat: bool) {
        let (size, transform, check, ext) = match &self.photos[&k] {
            PhotoState::Blob { chunk, size, orig_check } => (*size, TRANSFORM_LEPTON, *orig_check, vec![(*chunk, 1)]),
            PhotoState::Raw { ext, size } => (*size, TRANSFORM_NONE, 0, ext.clone()),
            PhotoState::Queued(_) => unreachable!(),
        };
        self.finish_file(item, size, transform, check, &ext);
        self.read_done += size;
        if repeat {
            self.stats.dup_bytes += size;
        }
    }
}

impl Deliver<Out, Error> for W<'_> {
    fn deliver(&mut self, o: Out) -> Result<()> {
        match o {
            Out::Pack(r) => {
                let p = r?;
                self.cells.pop_front();
                self.in_pool -= p.raw_len;
                self.write_pack(p)?;
                self.tick(false)
            }
            Out::Shrunk(blob) => self.photo_done(Some(blob), None),
            Out::Keep(data) => self.photo_done(None, Some(data)),
        }
    }
    fn idle(&mut self) -> Result<()> {
        self.tick(false)
    }
}

/// Sets the stop flag when dropped, so workers quit early on any way out (error, Stop, panic).
struct StopOnDrop<'a>(&'a AtomicBool);
impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

struct Planned {
    item: usize,
    kind: Kind,
    ext: String,
}

pub(crate) fn write_purr(items: &[Item], part: &Path, opts: &PurrOptions, progress: &dyn Progress, skipped: &mut Vec<(String, String)>) -> Result<Stats> {
    let low = opts.low_memory;
    let t = tuning(opts.level, low);
    let total: u64 = items.iter().map(|i| i.size).sum();

    // The lock, if any.
    let password = opts.password.as_deref().filter(|p| !p.is_empty());
    let (kdf, key) = match password {
        Some(pw) => {
            let kdf = seal::new_kdf()?;
            let key = seal::derive(pw, &kdf)?;
            (Some(kdf), Some(key))
        }
        None => (None, None),
    };
    let mut header = Header { major: VERSION_MAJOR, minor: VERSION_MINOR, kdf, key_check: [0; 16] };
    if let Some(k) = &key {
        header.key_check = k.header_check(Header::check_prefix(&header.to_bytes()));
    }

    // 1. Look at every file and sort by kind.
    let mut plan: Vec<Planned> = Vec::new();
    let mut metas: Vec<Meta> = Vec::with_capacity(items.len());
    let mut head = vec![0u8; 256 * 1024];
    for (i, it) in items.iter().enumerate() {
        metas.push(meta_of(&it.src));
        if it.is_dir {
            continue;
        }
        if i % 64 == 0 {
            if progress.cancelled() {
                return Err(Error::Cancelled);
            }
            progress.update(0, total, &it.name);
        }
        let n = match File::open(&it.src).and_then(|mut f| crate::read_up_to(&mut f, &mut head)) {
            Ok(n) => n,
            Err(e) => {
                skipped.push((it.name.clone(), format!("couldn't read it ({})", e.kind())));
                continue;
            }
        };
        let mut kind = codec::classify(&it.name, it.size, &head[..n]);
        if kind == Kind::Photo && (!t.photos || it.size > jpeg::MAX_JPEG_BYTES) {
            kind = Kind::Packed;
        }
        if opts.level == Level::Store && kind != Kind::Packed {
            kind = Kind::Data;
        }
        plan.push(Planned { item: i, kind, ext: codec::ext_of(&it.name) });
    }
    drop(head);
    let phase = |k: Kind| match k {
        Kind::Text => 0u8,
        Kind::Exec(a) => 1 + a.min(2),
        Kind::Data => 4,
        Kind::Packed => 5,
        Kind::Photo => 6,
    };
    // Within a kind: by file type, then by full path, so files from the same folder stay together
    // (measured: clearly better than sorting by file name alone).
    plan.sort_by(|a, b| (phase(a.kind), &a.ext, &items[a.item].name).cmp(&(phase(b.kind), &b.ext, &items[b.item].name)));

    // 2 + 3. Chunk, find repeats, squeeze, write.
    let file = fs::OpenOptions::new().write(true).create_new(true).open(part)?;
    let stop = AtomicBool::new(false);
    let mut w = W {
        out: BufWriter::with_capacity(1 << 20, file),
        offset: HEADER_LEN as u64,
        progress,
        stop: &stop,
        total,
        pack_bytes: t.pack_bytes,
        packs: Vec::new(),
        chunks: Vec::new(),
        seen: HashMap::new(),
        cur: Vec::new(),
        cur_first: 0,
        items,
        metas,
        files: (0..items.len()).map(|_| None).collect(),
        extents: Vec::new(),
        stats: Stats::default(),
        read_done: 0,
        in_pool: 0,
        cells: VecDeque::new(),
        last_report: 0,
        current: String::new(),
        photo_queue: VecDeque::new(),
        photos: HashMap::new(),
        chunker: Chunker::default(),
        cur_recipes: Vec::new(),
        pending: VecDeque::new(),
    };
    w.out.write_all(&header.to_bytes())?;
    for (i, it) in items.iter().enumerate() {
        if it.is_dir {
            let m = &w.metas[i];
            w.files[i] = Some(FileInfo { path: it.name.clone(), is_dir: true, size: 0, mtime_ms: m.mtime_ms, mode: m.mode, transform: 0, orig_check: 0, ext_start: 0, ext_count: 0 });
        }
    }

    let key_ref = key.as_ref();
    let max_pixels = t.max_pixels;
    let stop_ref = &stop;
    let work = move |job: Job| -> Out {
        let r = catch_unwind(AssertUnwindSafe(|| match job {
            Job::Pack { raw, recipes, first_chunk, chunk_count, cell } => Out::Pack((|| {
                let (codec, squeezed) = squeeze(&raw, &recipes, &cell, stop_ref)?;
                let raw_len = raw.len() as u64;
                drop(raw);
                let stored = match key_ref {
                    Some(k) => k.seal(&squeezed)?,
                    None => squeezed,
                };
                Ok(PackOut { codec, stored, raw_len, first_chunk, chunk_count })
            })()),
            Job::Photo { data } => {
                if stop_ref.load(Ordering::Relaxed) {
                    return Out::Keep(data);
                }
                match jpeg::shrink(&data, max_pixels) {
                    Some(blob) => Out::Shrunk(blob),
                    None => Out::Keep(data),
                }
            }
        }));
        r.unwrap_or_else(|_| Out::Pack(Err(Error::Other("Something went wrong inside Purrzip while packing.".into()))))
    };

    /// Hand queued packs to the workers (results come back, in order, through `w`).
    fn hand_over<'s>(w: &mut W, pool: &mut Pool<'s, Job, Out>) -> Result<()> {
        while let Some(job) = w.pending.pop_front() {
            pool.submit(job, w)?;
        }
        Ok(())
    }

    let result: Result<()> = std::thread::scope(|s| {
        let _guard = StopOnDrop(&stop);
        let mut pack_pool: Pool<Job, Out> = Pool::new(s, t.pack_workers, t.pack_workers + 1, &work);
        let mut photo_pool: Pool<Job, Out> = Pool::new(s, t.photo_workers, t.photo_workers * 2, &work);
        let mut cur_kind: Option<Kind> = None;

        for p in &plan {
            if cur_kind != Some(p.kind) {
                // A new kind starts a new pack.
                if cur_kind == Some(Kind::Photo) {
                    photo_pool.drain(&mut w)?;
                }
                w.roll_pack();
                hand_over(&mut w, &mut pack_pool)?;
                cur_kind = Some(p.kind);
                w.cur_recipes = recipes(opts.level, low, p.kind);
                w.pack_bytes = if matches!(p.kind, Kind::Photo | Kind::Packed) { t.store_pack_bytes } else { t.pack_bytes };
            }
            let it = &items[p.item];
            w.current = it.name.clone();
            w.tick(false)?;

            if p.kind == Kind::Photo {
                let data = match fs::read(&it.src) {
                    Ok(d) => d,
                    Err(e) => {
                        skipped.push((it.name.clone(), format!("couldn't read it ({})", e.kind())));
                        continue;
                    }
                };
                let h = blake3::hash(&data);
                let k = key128(&h);
                match w.photos.get(&k) {
                    Some(PhotoState::Queued(_)) => {
                        if let Some(PhotoState::Queued(twins)) = w.photos.get_mut(&k) {
                            twins.push(p.item);
                        }
                    }
                    Some(_) => w.photo_file(p.item, k, true),
                    None => {
                        w.photos.insert(k, PhotoState::Queued(Vec::new()));
                        let orig_check = u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap());
                        w.photo_queue.push_back((k, p.item, data.len() as u64, orig_check));
                        photo_pool.submit(Job::Photo { data }, &mut w)?;
                        hand_over(&mut w, &mut pack_pool)?;
                    }
                }
                continue;
            }

            let mut f = match File::open(&it.src) {
                Ok(f) => f,
                Err(e) => {
                    skipped.push((it.name.clone(), format!("couldn't read it ({})", e.kind())));
                    continue;
                }
            };
            let mut ext: Vec<(u32, u32)> = Vec::new();
            let mut chunker = std::mem::take(&mut w.chunker);
            let r = chunker.run(&mut f, &mut |ch| {
                let (id, new) = w.add_chunk(ch)?;
                if !new {
                    w.stats.dup_bytes += ch.len() as u64;
                }
                push_extent(&mut ext, id);
                w.read_done += ch.len() as u64;
                if w.pack_full() {
                    w.roll_pack();
                    hand_over(&mut w, &mut pack_pool)?;
                }
                w.tick(false)
            });
            w.chunker = chunker;
            let size = r?;
            w.finish_file(p.item, size, TRANSFORM_NONE, 0, &ext);
        }
        photo_pool.drain(&mut w)?;
        w.roll_pack();
        hand_over(&mut w, &mut pack_pool)?;
        pack_pool.drain(&mut w)?;
        Ok(())
    });
    result?;

    // 4. Table of contents + footer.
    w.current = String::new();
    let mut chunks = Vec::with_capacity(w.chunks.len());
    for (pi, p) in w.packs.iter().enumerate() {
        let mut off = 0u64;
        for c in &w.chunks[p.first_chunk as usize..(p.first_chunk + p.chunk_count) as usize] {
            chunks.push(ChunkInfo { pack: pi as u32, off, len: c.len, check: c.check });
            off += c.len;
        }
    }
    debug_assert_eq!(chunks.len(), w.chunks.len());
    let created_s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let index = Index {
        creator: format!("Purrzip {}", env!("CARGO_PKG_VERSION")),
        created_s,
        packs: std::mem::take(&mut w.packs),
        chunks,
        files: w.files.iter_mut().filter_map(|f| f.take()).collect(),
        extents: std::mem::take(&mut w.extents),
        stats: Some(w.stats.clone()),
    };
    let raw = index.to_bytes();
    let level = if raw.len() > 16 << 20 { 9 } else { 19 };
    let squeezed = codec::compress(&Recipe::Zstd { level, long: false }, &raw)?;
    let stored = match &key {
        Some(k) => k.seal(&squeezed)?,
        None => squeezed,
    };
    w.out.write_all(&stored)?;
    let footer = Footer { index_offset: w.offset, index_stored: stored.len() as u64, index_raw: raw.len() as u64, index_codec: CODEC_ZSTD, index_hash: if key.is_some() { [0; 16] } else { index_hash(&raw) } };
    w.out.write_all(&footer.to_bytes())?;
    w.out.flush()?;
    w.out.get_ref().sync_all()?;
    let stats = w.stats.clone();
    drop(w);

    if opts.repair {
        progress.update(total, total, "Adding repair data…");
        super::recovery::append(part, super::recovery::DEFAULT_PERCENT, progress)?;
    }
    Ok(stats)
}

/// Read helper for tests and tools: how many bytes a reader yields.
#[allow(dead_code)]
pub(crate) fn count_bytes(r: &mut dyn Read) -> std::io::Result<u64> {
    std::io::copy(r, &mut std::io::sink())
}
