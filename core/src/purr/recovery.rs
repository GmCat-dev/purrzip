//! Repair data: lets a damaged .purr fix itself.
//!
//! The archive body is cut into equal sectors. Sectors are dealt out round-robin into groups,
//! and every group gets a few Reed-Solomon parity sectors (about 3% extra). A checksum per
//! sector tells which ones are bad; a group can rebuild as many bad sectors as it has parity.
//! Because neighbouring sectors belong to different groups, one damaged stretch is spread thin
//! over many groups: a single hole of up to about 3% of the archive can be repaired, and so can
//! many small ones.
//!
//! Layout after the body:  head | checksum table | parity sectors | checksum table again | tail
//! The table is stored twice (each block with its own checksum) and the settings are in both the
//! head and the tail, so damage to the repair data itself is survivable too.
//!
//! Repair never touches the original file: it writes a repaired copy.

use crate::{Error, Progress, Result};
use reed_solomon_simd::{ReedSolomonDecoder, ReedSolomonEncoder};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const DEFAULT_PERCENT: u32 = 3;
const HEAD_MAGIC: [u8; 8] = *b"PURRRECH";
const TAIL_MAGIC: [u8; 8] = *b"PURRRECT";
pub const REC_LEN: u64 = 64;
/// Data per group (bounds memory while making and using repair data).
const GROUP_BYTES: u64 = 32 << 20;
const MIN_SECTOR: u64 = 1024;
const MAX_SECTOR: u64 = 512 * 1024;
const TARGET_SECTORS: u64 = 50_000;
const MAX_SHARDS: u64 = 32768;
const TABLE_BLOCK: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    pub body_len: u64,
    pub sector: u64,
    /// Number of data sectors.
    pub sectors: u64,
    pub groups: u64,
    /// Parity sectors per group.
    pub parity: u64,
}

impl Geometry {
    pub fn plan(body_len: u64, percent: u32) -> Geometry {
        let sector = body_len.div_ceil(TARGET_SECTORS).max(1).next_power_of_two().clamp(MIN_SECTOR, MAX_SECTOR);
        let sectors = body_len.div_ceil(sector).max(1);
        let k_max = (GROUP_BYTES / sector).min(MAX_SHARDS);
        let groups = sectors.div_ceil(k_max).max(1);
        let k = sectors.div_ceil(groups);
        let parity = (k * percent as u64).div_ceil(100).max(if sectors >= 2 { 2 } else { 1 }).min(MAX_SHARDS);
        Geometry { body_len, sector, sectors, groups, parity }
    }

    /// Data sectors in group `g` (sector i belongs to group i % groups).
    fn k_of(&self, g: u64) -> u64 {
        (self.sectors - g).div_ceil(self.groups)
    }
    fn entries(&self) -> u64 {
        self.sectors + self.groups * self.parity
    }
    fn table_len(&self) -> u64 {
        let e = self.entries();
        e * 4 + e.div_ceil(TABLE_BLOCK as u64) * 4
    }
    fn table_a(&self) -> u64 {
        self.body_len + REC_LEN
    }
    fn parity_offset(&self) -> u64 {
        self.table_a() + self.table_len()
    }
    fn table_b(&self) -> u64 {
        self.parity_offset() + self.groups * self.parity * self.sector
    }
    fn tail(&self) -> u64 {
        self.table_b() + self.table_len()
    }
    pub fn total_len(&self) -> u64 {
        self.tail() + REC_LEN
    }

    fn to_bytes(&self, magic: &[u8; 8]) -> [u8; REC_LEN as usize] {
        let mut b = [0u8; REC_LEN as usize];
        b[..8].copy_from_slice(magic);
        b[8..12].copy_from_slice(&1u32.to_le_bytes());
        b[12..16].copy_from_slice(&(self.sector as u32).to_le_bytes());
        b[16..24].copy_from_slice(&self.body_len.to_le_bytes());
        b[24..32].copy_from_slice(&self.sectors.to_le_bytes());
        b[32..40].copy_from_slice(&self.groups.to_le_bytes());
        b[40..48].copy_from_slice(&self.parity.to_le_bytes());
        let crc = crc32fast::hash(&b[..60]);
        b[60..64].copy_from_slice(&crc.to_le_bytes());
        b
    }

    fn parse(b: &[u8], magic: &[u8; 8]) -> Option<Geometry> {
        if b.len() < REC_LEN as usize || &b[..8] != magic {
            return None;
        }
        if u32::from_le_bytes(b[60..64].try_into().unwrap()) != crc32fast::hash(&b[..60]) {
            return None;
        }
        if u32::from_le_bytes(b[8..12].try_into().unwrap()) != 1 {
            return None;
        }
        let u = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        let g = Geometry { sector: u32::from_le_bytes(b[12..16].try_into().unwrap()) as u64, body_len: u(16), sectors: u(24), groups: u(32), parity: u(40) };
        // Refuse settings that make no sense or would need unreasonable memory.
        let ok = g.sector >= 2
            && g.sector % 2 == 0
            && g.sector <= 16 << 20
            && g.body_len > 0
            && g.sectors == g.body_len.div_ceil(g.sector)
            && g.groups >= 1
            && g.groups <= g.sectors
            && g.parity >= 1
            && g.parity <= MAX_SHARDS
            && g.sectors.div_ceil(g.groups) <= MAX_SHARDS
            && g.sectors.div_ceil(g.groups) * g.sector <= 256 << 20
            && g.body_len < 1 << 60;
        if ok { Some(g) } else { None }
    }
}

fn rs_err(e: reed_solomon_simd::Error) -> Error {
    Error::Other(format!("repair maths: {}", e))
}

/// Read sector `i` of the body (zero-padded at the end of the body). Returns false if it can't be read in full.
fn read_data_sector(f: &mut File, geo: &Geometry, i: u64, buf: &mut [u8]) -> bool {
    let start = i * geo.sector;
    let want = (geo.body_len - start).min(geo.sector) as usize;
    buf[want..].fill(0);
    f.seek(SeekFrom::Start(start)).is_ok() && f.read_exact(&mut buf[..want]).is_ok()
}

fn table_bytes(crcs: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(crcs.len() * 4 + crcs.len() / TABLE_BLOCK * 4 + 4);
    for block in crcs.chunks(TABLE_BLOCK) {
        let start = out.len();
        for c in block {
            out.extend_from_slice(&c.to_le_bytes());
        }
        let crc = crc32fast::hash(&out[start..]);
        out.extend_from_slice(&crc.to_le_bytes());
    }
    out
}

/// Add repair data to the end of a finished archive.
pub fn append(path: &Path, percent: u32, progress: &dyn Progress) -> Result<Geometry> {
    let body_len = std::fs::metadata(path)?.len();
    let geo = Geometry::plan(body_len, percent.clamp(1, 50));
    append_geo(path, &geo, progress)?;
    Ok(geo)
}

/// Write the repair data for a body laid out as `geo` says (the file must be exactly the body).
fn append_geo(path: &Path, geo: &Geometry, progress: &dyn Progress) -> Result<()> {
    let mut rd = File::open(path)?;
    let body_len = rd.metadata()?.len();
    if body_len != geo.body_len {
        return Err(Error::Other("The archive changed while repair data was being added.".into()));
    }
    let geo = geo.clone();
    let s = geo.sector as usize;
    let mut wr = OpenOptions::new().write(true).open(path)?;
    let mut crcs = vec![0u32; geo.entries() as usize];
    let mut buf = vec![0u8; s];
    let mut enc: Option<ReedSolomonEncoder> = None;
    let mut done = 0u64;
    for g in 0..geo.groups {
        if progress.cancelled() {
            return Err(Error::Cancelled);
        }
        let k = geo.k_of(g) as usize;
        match enc.as_mut() {
            Some(e) => e.reset(k, geo.parity as usize, s).map_err(rs_err)?,
            None => enc = Some(ReedSolomonEncoder::new(k, geo.parity as usize, s).map_err(rs_err)?),
        }
        let e = enc.as_mut().unwrap();
        for r in 0..k as u64 {
            let i = g + r * geo.groups;
            if !read_data_sector(&mut rd, &geo, i, &mut buf) {
                return Err(Error::Other("Couldn't read the archive back to add repair data.".into()));
            }
            crcs[i as usize] = crc32fast::hash(&buf);
            e.add_original_shard(&buf).map_err(rs_err)?;
            done += s as u64;
            if r % 256 == 0 {
                progress.update(done.min(body_len), body_len, "Adding repair data…");
            }
        }
        let result = e.encode().map_err(rs_err)?;
        wr.seek(SeekFrom::Start(geo.parity_offset() + g * geo.parity * geo.sector))?;
        for (j, shard) in result.recovery_iter().enumerate() {
            crcs[(geo.sectors + g * geo.parity) as usize + j] = crc32fast::hash(shard);
            wr.write_all(shard)?;
        }
    }
    let table = table_bytes(&crcs);
    debug_assert_eq!(table.len() as u64, geo.table_len());
    wr.seek(SeekFrom::Start(geo.body_len))?;
    wr.write_all(&geo.to_bytes(&HEAD_MAGIC))?;
    wr.write_all(&table)?;
    wr.seek(SeekFrom::Start(geo.table_b()))?;
    wr.write_all(&table)?;
    wr.write_all(&geo.to_bytes(&TAIL_MAGIC))?;
    wr.flush()?;
    wr.sync_all()?;
    Ok(())
}

/// The quick check: is there an intact repair-data tail at the very end of the file?
pub fn locate_tail(f: &mut File) -> Option<Geometry> {
    let len = f.metadata().ok()?.len();
    let mut b = [0u8; REC_LEN as usize];
    if len >= REC_LEN && f.seek(SeekFrom::Start(len - REC_LEN)).is_ok() && f.read_exact(&mut b).is_ok() {
        if let Some(g) = Geometry::parse(&b, &TAIL_MAGIC) {
            if g.total_len() == len {
                return Some(g);
            }
        }
    }
    None
}

/// Find the repair settings of a file, if it has repair data (searches when the end is damaged).
pub fn locate(f: &mut File) -> Option<Geometry> {
    if let Some(g) = locate_tail(f) {
        return Some(g);
    }
    let len = f.metadata().ok()?.len();
    // Damaged or cut-off end: look for the head (or a tail that isn't at the end any more).
    // Repair data is about 3% of the file, so the head is normally in the last tenth; if that
    // fails, search everything.
    let mut found = scan_for_marker(f, len, len.saturating_sub(len / 8 + (4 << 20)));
    if found.is_none() {
        found = scan_for_marker(f, len, 0);
    }
    found
}

fn scan_for_marker(f: &mut File, len: u64, from: u64) -> Option<Geometry> {
    const WIN: usize = 1 << 20;
    let mut buf = vec![0u8; WIN + REC_LEN as usize];
    let mut pos = from;
    let mut best = None;
    while pos < len {
        let n = ((len - pos) as usize).min(buf.len());
        if f.seek(SeekFrom::Start(pos)).is_err() || f.read_exact(&mut buf[..n]).is_err() {
            return best;
        }
        let mut i = 0;
        while i + REC_LEN as usize <= n {
            if buf[i] == b'P' && buf[i..].starts_with(b"PURRREC") {
                let at = pos + i as u64;
                if let Some(g) = Geometry::parse(&buf[i..i + REC_LEN as usize], &HEAD_MAGIC) {
                    if g.body_len == at {
                        return Some(g);
                    }
                }
                if let Some(g) = Geometry::parse(&buf[i..i + REC_LEN as usize], &TAIL_MAGIC) {
                    if g.tail() == at {
                        best = Some(g);
                    }
                }
            }
            i += 1;
        }
        if n < buf.len() {
            break;
        }
        pos += WIN as u64;
    }
    best
}

pub fn has_recovery(path: &Path) -> bool {
    File::open(path).ok().and_then(|mut f| locate(&mut f)).is_some()
}

/// Checksums from the two table copies; None where both copies of a block are damaged.
fn load_table(f: &mut File, geo: &Geometry) -> Vec<Option<u32>> {
    let entries = geo.entries() as usize;
    let read_copy = |f: &mut File, at: u64| -> Vec<u8> {
        let mut v = vec![0u8; geo.table_len() as usize];
        let mut got = 0;
        if f.seek(SeekFrom::Start(at)).is_ok() {
            while got < v.len() {
                match f.read(&mut v[got..]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got += n,
                }
            }
        }
        v.truncate(got);
        v
    };
    let (a, b) = (read_copy(f, geo.table_a()), read_copy(f, geo.table_b()));
    let mut out = vec![None; entries];
    let mut pos = 0usize; // byte position inside a copy
    let mut e = 0usize;
    while e < entries {
        let n = (entries - e).min(TABLE_BLOCK);
        let end = pos + n * 4 + 4;
        for copy in [&a, &b] {
            if copy.len() >= end && u32::from_le_bytes(copy[end - 4..end].try_into().unwrap()) == crc32fast::hash(&copy[pos..end - 4]) {
                for (j, c) in copy[pos..end - 4].chunks_exact(4).enumerate() {
                    out[e + j] = Some(u32::from_le_bytes(c.try_into().unwrap()));
                }
                break;
            }
        }
        pos = end;
        e += n;
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct RepairReport {
    /// Sectors of the archive itself that were damaged.
    pub damaged: u64,
    /// Damaged sectors of the repair data.
    pub damaged_repair_data: u64,
    pub sector_bytes: u64,
    pub repaired: bool,
    /// True when nothing was wrong (no repaired copy is written).
    pub clean: bool,
}

struct Scan {
    geo: Geometry,
    table: Vec<Option<u32>>,
    /// Per data sector / per parity sector: is it known to be good?
    data_ok: Vec<bool>,
    parity_ok: Vec<bool>,
}

fn scan(f: &mut File, progress: &dyn Progress) -> Result<Scan> {
    let geo = locate(f).ok_or_else(|| Error::Other("This archive has no repair data.".into()))?;
    let table = load_table(f, &geo);
    let s = geo.sector as usize;
    let mut buf = vec![0u8; s];
    let mut data_ok = vec![false; geo.sectors as usize];
    for i in 0..geo.sectors {
        if i % 512 == 0 {
            if progress.cancelled() {
                return Err(Error::Cancelled);
            }
            progress.update(i * geo.sector, geo.body_len * 2, "Checking the archive…");
        }
        let readable = read_data_sector(f, &geo, i, &mut buf);
        data_ok[i as usize] = readable && table[i as usize] == Some(crc32fast::hash(&buf));
    }
    let mut parity_ok = vec![false; (geo.groups * geo.parity) as usize];
    for j in 0..geo.groups * geo.parity {
        let readable = f.seek(SeekFrom::Start(geo.parity_offset() + j * geo.sector)).is_ok() && f.read_exact(&mut buf).is_ok();
        parity_ok[j as usize] = readable && table[(geo.sectors + j) as usize] == Some(crc32fast::hash(&buf));
    }
    Ok(Scan { geo, table, data_ok, parity_ok })
}

/// Check an archive against its repair data without changing anything.
pub fn verify(path: &Path, progress: &dyn Progress) -> Result<RepairReport> {
    let mut f = File::open(path)?;
    let sc = scan(&mut f, progress)?;
    let damaged = sc.data_ok.iter().filter(|ok| !**ok).count() as u64;
    let damaged_repair_data = sc.parity_ok.iter().filter(|ok| !**ok).count() as u64;
    let whole = f.metadata()?.len() == sc.geo.total_len() && sc.table.iter().all(|t| t.is_some());
    Ok(RepairReport { damaged, damaged_repair_data, sector_bytes: sc.geo.sector, repaired: false, clean: damaged == 0 && damaged_repair_data == 0 && whole })
}

/// Rebuild the damaged parts of `src` into a new file `dst`. `src` is never changed.
/// If there is more damage than the repair data covers, nothing is written and an error is returned.
pub fn repair(src: &Path, dst: &Path, progress: &dyn Progress) -> Result<RepairReport> {
    let mut f = File::open(src)?;
    let sc = scan(&mut f, progress)?;
    let geo = sc.geo.clone();
    let damaged = sc.data_ok.iter().filter(|ok| !**ok).count() as u64;
    let damaged_repair_data = sc.parity_ok.iter().filter(|ok| !**ok).count() as u64;
    let mut report = RepairReport { damaged, damaged_repair_data, sector_bytes: geo.sector, repaired: false, clean: false };
    let whole = f.metadata()?.len() == geo.total_len() && sc.table.iter().all(|t| t.is_some());
    if damaged == 0 && damaged_repair_data == 0 && whole {
        report.clean = true;
        return Ok(report);
    }
    // Can every group be rebuilt?
    for g in 0..geo.groups {
        let bad = (0..geo.k_of(g)).filter(|r| !sc.data_ok[(g + r * geo.groups) as usize]).count();
        let spare = (0..geo.parity).filter(|j| sc.parity_ok[(g * geo.parity + j) as usize]).count();
        if bad > spare {
            return Err(Error::Damaged("there is more damage than the repair data can fix".into()));
        }
    }
    // Copy the body, then patch the bad sectors in the copy.
    let s = geo.sector as usize;
    let result = (|| -> Result<()> {
        let mut out = OpenOptions::new().read(true).write(true).create_new(true).open(dst)?;
        let mut buf = vec![0u8; s];
        for i in 0..geo.sectors {
            if i % 512 == 0 && progress.cancelled() {
                return Err(Error::Cancelled);
            }
            let want = (geo.body_len - i * geo.sector).min(geo.sector) as usize;
            if !(sc.data_ok[i as usize] && read_data_sector(&mut f, &geo, i, &mut buf)) {
                buf.fill(0); // placeholder, rebuilt below
            }
            out.write_all(&buf[..want])?;
        }
        let mut dec: Option<ReedSolomonDecoder> = None;
        let mut fixed = 0u64;
        for g in 0..geo.groups {
            let k = geo.k_of(g);
            let bad: Vec<u64> = (0..k).filter(|r| !sc.data_ok[(g + r * geo.groups) as usize]).collect();
            if bad.is_empty() {
                continue;
            }
            if progress.cancelled() {
                return Err(Error::Cancelled);
            }
            match dec.as_mut() {
                Some(d) => d.reset(k as usize, geo.parity as usize, s).map_err(rs_err)?,
                None => dec = Some(ReedSolomonDecoder::new(k as usize, geo.parity as usize, s).map_err(rs_err)?),
            }
            let d = dec.as_mut().unwrap();
            for r in 0..k {
                let i = g + r * geo.groups;
                if sc.data_ok[i as usize] {
                    if !read_data_sector(&mut f, &geo, i, &mut buf) {
                        return Err(Error::Damaged("the archive changed while it was being repaired".into()));
                    }
                    d.add_original_shard(r as usize, &buf).map_err(rs_err)?;
                }
            }
            let mut added = 0;
            for j in 0..geo.parity {
                if added == bad.len() {
                    break;
                }
                let pj = g * geo.parity + j;
                if sc.parity_ok[pj as usize] {
                    f.seek(SeekFrom::Start(geo.parity_offset() + pj * geo.sector))?;
                    f.read_exact(&mut buf)?;
                    d.add_recovery_shard(j as usize, &buf).map_err(rs_err)?;
                    added += 1;
                }
            }
            let restored = d.decode().map_err(rs_err)?;
            for r in &bad {
                let i = g + r * geo.groups;
                let sector = restored.restored_original(*r as usize).ok_or_else(|| Error::Damaged("a damaged part could not be rebuilt".into()))?;
                // Where the original checksum survived, the rebuilt sector must match it.
                if let Some(c) = sc.table[i as usize] {
                    if c != crc32fast::hash(sector) {
                        return Err(Error::Damaged("a rebuilt part didn't check out".into()));
                    }
                }
                let want = (geo.body_len - i * geo.sector).min(geo.sector) as usize;
                out.seek(SeekFrom::Start(i * geo.sector))?;
                out.write_all(&sector[..want])?;
                fixed += 1;
            }
            progress.update(geo.body_len + fixed * geo.sector, geo.body_len * 2, "Repairing…");
        }
        out.flush()?;
        out.sync_all()?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(dst);
        return Err(e);
    }
    // Fresh repair data for the repaired copy, laid out exactly as before.
    if let Err(e) = append_geo(dst, &geo, progress) {
        let _ = std::fs::remove_file(dst);
        return Err(e);
    }
    report.repaired = true;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Quiet;

    fn noise(n: usize, seed: u64) -> Vec<u8> {
        let mut x = seed | 1;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn geometry_makes_sense_at_every_size() {
        for len in [1u64, 500, 1024, 100_000, 10 << 20, 100 << 20, 1 << 30, 10 << 30, 100 << 30, 1 << 40] {
            let g = Geometry::plan(len, 3);
            assert_eq!(g.sectors, len.div_ceil(g.sector).max(1));
            let k = g.sectors.div_ceil(g.groups);
            assert!(k <= MAX_SHARDS && g.parity <= MAX_SHARDS, "{:?}", g);
            assert!(k * g.sector <= GROUP_BYTES.max(g.sector), "{:?}", g);
            assert_eq!((0..g.groups).map(|x| g.k_of(x)).sum::<u64>(), g.sectors);
            assert_eq!(Geometry::parse(&g.to_bytes(&TAIL_MAGIC), &TAIL_MAGIC), Some(g.clone()));
            if len >= 10 << 20 {
                let overhead = (g.groups * g.parity * g.sector) as f64 / len as f64;
                assert!(overhead < 0.05, "{} overhead {:?}", overhead, g);
            }
        }
    }

    struct Case {
        dir: std::path::PathBuf,
        orig: Vec<u8>,
        geo: Geometry,
    }
    fn case(len: usize, seed: u64) -> Case {
        let dir = crate::temp_folder("rectest").unwrap();
        let body = noise(len, seed);
        std::fs::write(dir.join("a.purr"), &body).unwrap();
        let geo = append(&dir.join("a.purr"), 3, &Quiet::default()).unwrap();
        let orig = std::fs::read(dir.join("a.purr")).unwrap();
        assert_eq!(orig.len() as u64, geo.total_len());
        assert_eq!(&orig[..len], &body[..]);
        Case { dir, orig, geo }
    }
    impl Case {
        /// Damage the file with `f`, repair, and say whether the body came back.
        fn try_repair(&self, f: impl FnOnce(&mut Vec<u8>)) -> std::result::Result<bool, Error> {
            let mut bytes = self.orig.clone();
            f(&mut bytes);
            let src = self.dir.join("damaged.purr");
            let dst = self.dir.join("fixed.purr");
            let _ = std::fs::remove_file(&dst);
            std::fs::write(&src, &bytes).unwrap();
            let r = repair(&src, &dst, &Quiet::default())?;
            if r.clean {
                return Ok(bytes[..self.geo.body_len as usize] == self.orig[..self.geo.body_len as usize]);
            }
            let fixed = std::fs::read(&dst).unwrap();
            // the repaired copy must be whole again, repair data included
            assert!(verify(&dst, &Quiet::default()).unwrap().clean);
            Ok(fixed == self.orig)
        }
    }
    impl Drop for Case {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn untouched_file_is_clean() {
        let c = case(300_000, 1);
        assert!(verify(&c.dir.join("a.purr"), &Quiet::default()).unwrap().clean);
        assert!(has_recovery(&c.dir.join("a.purr")));
        assert_eq!(c.try_repair(|_| {}).unwrap(), true);
    }

    #[test]
    fn fixes_flipped_bits_anywhere() {
        let c = case(2_000_000, 2);
        let n = c.orig.len();
        // single flips at the start, in the middle, at the end of the body, in the table, in the parity
        for pos in [0usize, 5, 1023, 1024, 777_777, c.geo.body_len as usize - 1, c.geo.table_a() as usize + 10, c.geo.parity_offset() as usize + 5, c.geo.table_b() as usize + 3, n - 70] {
            assert_eq!(c.try_repair(|b| b[pos] ^= 0x10).unwrap(), true, "flip at {}", pos);
        }
        // many scattered flips, fewer than the parity count
        let flips = (c.geo.parity as usize - 1).min(40);
        assert_eq!(c.try_repair(|b| for i in 0..flips { b[(i * 48_611) % c.geo.body_len as usize] ^= 1; }).unwrap(), true);
    }

    #[test]
    fn fixes_a_hole_of_almost_three_percent() {
        let c = case(4_000_000, 3);
        let hole = (c.geo.parity as usize - 1) * c.geo.sector as usize;
        assert!(hole as f64 > 0.025 * 4_000_000.0, "hole {} too small to be a fair test", hole);
        for start in [0usize, 1_000_001, 4_000_000 - hole] {
            assert_eq!(c.try_repair(|b| b[start..start + hole].fill(0)).unwrap(), true, "hole at {}", start);
        }
    }

    #[test]
    fn striping_spreads_a_hole_over_groups() {
        // 80 MiB: several groups. A single 2 MiB hole (more than one group's parity could take
        // alone) is repaired because neighbouring sectors belong to different groups.
        let c = case(80 << 20, 4);
        assert!(c.geo.groups >= 3, "{:?}", c.geo);
        let per_group_alone = (c.geo.parity * c.geo.sector) as usize;
        let hole = 2 << 20;
        assert!(hole > per_group_alone, "hole {} vs one group's parity {}", hole, per_group_alone);
        assert_eq!(c.try_repair(|b| b[30_000_000..30_000_000 + hole].fill(0xEE)).unwrap(), true);
    }

    #[test]
    fn too_much_damage_fails_cleanly_and_writes_nothing() {
        let c = case(1_000_000, 5);
        let hole = (c.geo.parity as usize + 2) * c.geo.sector as usize;
        let r = c.try_repair(|b| b[100_000..100_000 + hole].fill(0));
        assert!(matches!(r, Err(Error::Damaged(_))), "{:?}", r.map(|_| ()));
        assert!(!c.dir.join("fixed.purr").exists());
    }

    #[test]
    fn survives_a_lost_tail_and_a_damaged_table_copy() {
        let c = case(1_500_000, 6);
        let n = c.orig.len();
        // tail marker destroyed
        assert_eq!(c.try_repair(|b| b[n - 64..].fill(0)).unwrap(), true);
        // tail marker cut off entirely, plus a flipped bit in the body
        assert_eq!(c.try_repair(|b| { b.truncate(n - 64); b[4242] ^= 0xFF; }).unwrap(), true);
        // first table copy wiped, plus body damage
        let (ta, tl) = (c.geo.table_a() as usize, c.geo.table_len() as usize);
        assert_eq!(c.try_repair(|b| { b[ta..ta + tl].fill(0x55); b[999] ^= 1; }).unwrap(), true);
        // head marker destroyed too (tail still fine)
        let h = c.geo.body_len as usize;
        assert_eq!(c.try_repair(|b| { b[h..h + 64].fill(0); b[10] ^= 1; }).unwrap(), true);
    }

    #[test]
    fn no_repair_data_is_reported() {
        let dir = crate::temp_folder("rectest").unwrap();
        std::fs::write(dir.join("plain.bin"), noise(5000, 9)).unwrap();
        assert!(!has_recovery(&dir.join("plain.bin")));
        assert!(repair(&dir.join("plain.bin"), &dir.join("out.bin"), &Quiet::default()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
