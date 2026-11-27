//! Split archives: one archive saved as numbered pieces (`name.7z.001`, `name.7z.002`, ...),
//! the same plain cut 7-Zip and WinRAR make and open. Any archive kind can be split this way.
//!
//! Opening: [`open`] gives one reader over all the pieces, so every format reads a split set
//! exactly like a single file, without joining the pieces on disk first.
//! Making: [`cut`] turns a finished archive into pieces from the end backwards, so it only needs
//! room for one extra piece (phones).
//!
//! Copyright (C) 2026 GmCat. AGPL-3.0-or-later (see LICENSE).

use crate::{Error, Progress, Result};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Smallest piece Purrzip makes. Smaller is never useful and would make thousands of files.
pub const MIN_PIECE: u64 = 1_000_000;
/// Most pieces in one set (7-Zip's 3-digit numbering).
pub const MAX_PIECES: u32 = 999;

/// If `path` is one piece of a split set (`anything.001`, `.002`, ...), return the base path
/// (the name without the number) and the number. Needs at least 3 digits, like 7-Zip.
pub fn piece_number(path: &Path) -> Option<(PathBuf, u32)> {
    let name = path.file_name()?.to_str()?;
    let dot = name.rfind('.')?;
    let num = &name[dot + 1..];
    if dot == 0 || num.len() < 3 || num.len() > 4 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = num.parse().ok()?;
    if n == 0 {
        return None;
    }
    Some((path.with_file_name(&name[..dot]), n))
}

fn piece_path(base: &Path, n: u32, width: usize) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(format!(".{:0width$}", n, width = width));
    PathBuf::from(s)
}

/// All the pieces of the set `path` belongs to, in order, starting from piece 1.
/// None if `path` isn't a piece. Err if a piece in the middle is missing.
pub fn pieces(path: &Path) -> Result<Option<Vec<PathBuf>>> {
    let Some((base, _)) = piece_number(path) else { return Ok(None) };
    let width = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.rsplit('.').next()).map(|s| s.len()).unwrap_or(3);
    let first = piece_path(&base, 1, width);
    if !first.is_file() {
        return Err(Error::PiecesMissing(format!(
            "This is one piece of a split archive, but the first piece ({}) isn't in the same folder.",
            first.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        )));
    }
    let mut out = vec![first];
    let mut n = 2;
    loop {
        let p = piece_path(&base, n, width);
        if !p.is_file() {
            break;
        }
        out.push(p);
        n += 1;
    }
    // A later piece with a gap before it means one in the middle is missing.
    for later in n + 1..n + 20 {
        if piece_path(&base, later, width).is_file() {
            return Err(Error::PiecesMissing(format!(
                "Piece {} of this split archive is missing. Put all the pieces in the same folder and try again.",
                n
            )));
        }
    }
    Ok(Some(out))
}

/// Short facts about a split set, for the screen.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SplitInfo {
    pub pieces: u32,
    /// Total size of all the pieces together.
    pub bytes: u64,
    /// Name of the first piece.
    pub first: String,
}

pub fn info(path: &Path) -> Option<SplitInfo> {
    let list = pieces(path).ok()??;
    let bytes = list.iter().map(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0)).sum();
    Some(SplitInfo { pieces: list.len() as u32, bytes, first: list[0].file_name()?.to_string_lossy().into_owned() })
}

/// A file to read: either one ordinary file or every piece of a split set read as one.
pub struct Source {
    files: Vec<File>,
    /// Start offset of each piece, plus the total length at the end.
    starts: Vec<u64>,
    pos: u64,
}

impl Source {
    pub fn len(&self) -> u64 {
        *self.starts.last().unwrap()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn pieces(&self) -> usize {
        self.files.len()
    }
    fn piece_at(&self, pos: u64) -> usize {
        // last piece whose start <= pos
        self.starts[..self.files.len()].partition_point(|&s| s <= pos).saturating_sub(1)
    }
}

/// Open `path` for reading; a piece of a split set opens the whole set.
pub fn open(path: &Path) -> io::Result<Source> {
    let list = match pieces(path) {
        Ok(Some(l)) => l,
        Ok(None) => vec![path.to_path_buf()],
        Err(e) => return Err(io::Error::new(io::ErrorKind::NotFound, e.to_string())),
    };
    let mut files = Vec::with_capacity(list.len());
    let mut starts = Vec::with_capacity(list.len() + 1);
    let mut at = 0u64;
    for p in &list {
        let f = File::open(p)?;
        let len = f.metadata()?.len();
        starts.push(at);
        at += len;
        files.push(f);
    }
    starts.push(at);
    Ok(Source { files, starts, pos: 0 })
}

/// True when `path` is a piece of a split set (and the set is complete enough to open).
pub fn is_split(path: &Path) -> bool {
    matches!(pieces(path), Ok(Some(_)))
}

impl Read for Source {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.pos >= self.len() {
            return Ok(0);
        }
        let i = self.piece_at(self.pos);
        let within = self.pos - self.starts[i];
        let left_in_piece = self.starts[i + 1] - self.pos;
        let want = (buf.len() as u64).min(left_in_piece) as usize;
        let f = &mut self.files[i];
        f.seek(SeekFrom::Start(within))?;
        let n = f.read(&mut buf[..want])?;
        if n == 0 && want > 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "a piece of the archive got shorter while reading"));
        }
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Source {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let len = self.len() as i128;
        let p: i128 = match to {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::End(d) => len + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if p < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"));
        }
        self.pos = p as u64;
        Ok(self.pos)
    }
}

/// The piece names `out` would be cut into (for checking nothing is in the way).
pub fn piece_names(out: &Path, count: u32) -> Vec<PathBuf> {
    (1..=count).map(|n| piece_path(out, n, 3)).collect()
}

/// True if any `out.NNN` file already exists.
pub fn pieces_exist(out: &Path) -> bool {
    (1..=MAX_PIECES).any(|n| fs::symlink_metadata(piece_path(out, n, 3)).is_ok())
}

/// Cut the finished archive `whole` into pieces of `size` bytes named `out.001`, `out.002`, ...
/// Works from the end: each last piece is copied out and the big file shortened, so the extra
/// space needed is one piece. On failure every piece made is removed (the big file is then gone
/// too, being half cut; callers treat it as a temporary file).
/// Returns the piece paths in order.
pub fn cut(whole: &Path, out: &Path, size: u64, progress: &dyn Progress) -> Result<Vec<PathBuf>> {
    let total = fs::metadata(whole)?.len();
    let count = total.div_ceil(size).max(1);
    if count > MAX_PIECES as u64 {
        return Err(Error::Other(format!(
            "That would make {} pieces; the most is {}. Pick a bigger piece size.",
            count, MAX_PIECES
        )));
    }
    let count = count as u32;
    let names = piece_names(out, count);
    let mut made: Vec<PathBuf> = Vec::new();
    let r = (|| -> Result<()> {
        let mut src = fs::OpenOptions::new().read(true).write(true).open(whole)?;
        let mut buf = vec![0u8; 1 << 20];
        let mut copied = 0u64;
        for k in (1..count).rev() {
            let start = k as u64 * size;
            let end = total.min(start + size);
            let dst_path = &names[k as usize];
            let mut dst = fs::OpenOptions::new().write(true).create_new(true).open(dst_path)?;
            made.push(dst_path.clone());
            src.seek(SeekFrom::Start(start))?;
            let mut left = end - start;
            while left > 0 {
                if progress.cancelled() {
                    return Err(Error::Cancelled);
                }
                let n = (left as usize).min(buf.len());
                src.read_exact(&mut buf[..n])?;
                dst.write_all(&buf[..n])?;
                left -= n as u64;
                copied += n as u64;
                progress.update(copied, total, "Cutting into pieces");
            }
            dst.sync_all()?;
            drop(dst);
            src.set_len(start)?;
        }
        src.sync_all()?;
        drop(src);
        if fs::symlink_metadata(&names[0]).is_ok() {
            return Err(Error::Other("A file with that name appeared while packing; nothing was replaced.".into()));
        }
        fs::rename(whole, &names[0])?;
        made.push(names[0].clone());
        Ok(())
    })();
    if let Err(e) = r {
        for p in &made {
            let _ = fs::remove_file(p);
        }
        return Err(e);
    }
    Ok(names)
}

/// Parse a piece size the way people type it: "450 MB", "1.5 GB", "700m", "4g", "100000000".
/// MB and GB are 1,000,000 and 1,000,000,000 (what upload limits and cloud sites mean);
/// MiB/GiB are 1,048,576 and 1,073,741,824.
pub fn parse_size(s: &str) -> Option<u64> {
    let t = s.trim().to_ascii_lowercase().replace([',', '_'], "");
    let t = t.trim();
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
    let (num, unit) = (t[..split].trim(), t[split..].trim());
    let v: f64 = num.parse().ok()?;
    if !v.is_finite() || v <= 0.0 {
        return None;
    }
    let mult: f64 = match unit {
        "" | "b" | "bytes" => 1.0,
        "k" | "kb" => 1e3,
        "kib" => 1024.0,
        "m" | "mb" => 1e6,
        "mib" => 1048576.0,
        "g" | "gb" => 1e9,
        "gib" => 1073741824.0,
        _ => return None,
    };
    let b = (v * mult).floor();
    if b < 1.0 || b > 1e15 { None } else { Some(b as u64) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(piece_number(Path::new("/a/b.7z.001")), Some((PathBuf::from("/a/b.7z"), 1)));
        assert_eq!(piece_number(Path::new("x.zip.012")), Some((PathBuf::from("x.zip"), 12)));
        assert_eq!(piece_number(Path::new("x.zip.0001")), Some((PathBuf::from("x.zip"), 1)));
        assert_eq!(piece_number(Path::new("x.zip.01")), None);
        assert_eq!(piece_number(Path::new("x.zip.000")), None);
        assert_eq!(piece_number(Path::new("x.zip")), None);
        assert_eq!(piece_number(Path::new(".001")), None);
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("450 MB"), Some(450_000_000));
        assert_eq!(parse_size("1.5gb"), Some(1_500_000_000));
        assert_eq!(parse_size("700m"), Some(700_000_000));
        assert_eq!(parse_size("4 GiB"), Some(4 << 30));
        assert_eq!(parse_size("1,000"), Some(1000));
        assert_eq!(parse_size("0"), None);
        assert_eq!(parse_size("ten"), None);
        assert_eq!(parse_size("5 TB"), None);
    }

    #[test]
    fn cut_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let whole = dir.path().join("a.bin.part");
        let data: Vec<u8> = (0..(3 * MIN_PIECE + 12345)).map(|i| (i * 31 % 251) as u8).collect();
        fs::write(&whole, &data).unwrap();
        let out = dir.path().join("a.bin");
        let names = cut(&whole, &out, MIN_PIECE, &crate::Quiet::default()).unwrap();
        assert_eq!(names.len(), 4);
        assert!(!whole.exists());
        assert_eq!(fs::metadata(&names[3]).unwrap().len(), 12345);
        // read through every piece, from any of them
        for p in &names {
            let mut s = open(p).unwrap();
            assert_eq!(s.len(), data.len() as u64);
            let mut back = Vec::new();
            s.read_to_end(&mut back).unwrap();
            assert!(back == data);
        }
        // seeks across piece edges
        let mut s = open(&names[0]).unwrap();
        for &at in &[0u64, MIN_PIECE - 3, MIN_PIECE, 2 * MIN_PIECE + 7, data.len() as u64 - 10] {
            s.seek(SeekFrom::Start(at)).unwrap();
            let mut b = [0u8; 10];
            let n = crate::read_up_to(&mut s, &mut b).unwrap();
            assert_eq!(&b[..n], &data[at as usize..(at as usize + n)]);
        }
        s.seek(SeekFrom::End(-5)).unwrap();
        let mut b = Vec::new();
        s.read_to_end(&mut b).unwrap();
        assert_eq!(b, &data[data.len() - 5..]);
        // a missing middle piece is reported
        fs::remove_file(&names[1]).unwrap();
        assert!(pieces(&names[0]).is_err());
    }

    #[test]
    fn too_many_pieces() {
        let dir = tempfile::tempdir().unwrap();
        let whole = dir.path().join("b.part");
        let f = File::create(&whole).unwrap();
        f.set_len(1000 * 1000 + 1).unwrap();
        assert!(cut(&whole, &dir.path().join("b"), 1000, &crate::Quiet::default()).is_err());
        assert!(whole.exists(), "nothing touched when refused");
    }
}
