//! Purrzip core: open, list, extract and create archives.
//!
//! Copyright (C) 2026 GmCat. Licensed under the GNU AGPL v3 or later, with an
//! additional permission to combine with the UnRAR library (see LICENSE).

use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

mod create;
mod fmt_cab;
mod fmt_iso;
mod fmt_rar;
mod fmt_sevenz;
mod fmt_stream;
mod fmt_zip;
pub mod purr;
pub mod safe;
pub mod split;

pub use create::{create, CreateFormat, CreateOptions, Level};

/// Archive formats Purrzip can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Format {
    Zip,
    SevenZ,
    Rar,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
    Gz,
    Bz2,
    Xz,
    Zst,
    Iso,
    Cab,
    Purr,
}

impl Format {
    pub fn label(&self) -> &'static str {
        match self {
            Format::Zip => "ZIP",
            Format::SevenZ => "7z",
            Format::Rar => "RAR",
            Format::Tar => "TAR",
            Format::TarGz => "TAR.GZ",
            Format::TarBz2 => "TAR.BZ2",
            Format::TarXz => "TAR.XZ",
            Format::TarZst => "TAR.ZST",
            Format::Gz => "GZ",
            Format::Bz2 => "BZ2",
            Format::Xz => "XZ",
            Format::Zst => "ZST",
            Format::Iso => "ISO",
            Format::Cab => "CAB",
            Format::Purr => "PURR",
        }
    }
}

/// One item inside an archive. `index` is stable between `list` and `extract`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Entry {
    pub index: usize,
    pub path: String,
    pub size: u64,
    pub packed: Option<u64>,
    pub is_dir: bool,
    pub encrypted: bool,
    /// Seconds since 1970 (UTC), when known.
    pub modified: Option<i64>,
    /// Links and special files are listed but never written out.
    pub special: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Listing {
    pub format: Format,
    pub entries: Vec<Entry>,
    pub total_size: u64,
    pub any_encrypted: bool,
    /// True when sizes are estimates (single gz/bz2/xz/zst streams).
    pub size_unknown: bool,
    /// Set when the archive is saved as numbered pieces (name.7z.001, .002, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split: Option<split::SplitInfo>,
    /// Extra facts for .purr archives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purr: Option<purr::PurrInfo>,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Report {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    /// (name inside the archive, why it was skipped)
    pub skipped: Vec<(String, String)>,
    /// Names that were changed to be safe on this system: (original, written as)
    pub renamed: Vec<(String, String)>,
    /// Where each extracted entry ended up, relative to the destination: (index, path)
    pub written: Vec<(usize, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("This archive needs a password.")]
    NeedPassword,
    #[error("That password doesn't open this archive.")]
    WrongPassword,
    #[error("Purrzip doesn't recognise this kind of file.")]
    Unknown,
    #[error("This archive uses something Purrzip can't open yet: {0}")]
    Unsupported(String),
    #[error("This archive looks damaged: {0}")]
    Damaged(String),
    #[error("Stopped.")]
    Cancelled,
    /// A split archive with a piece missing (the message says which, in plain words).
    #[error("{0}")]
    PiecesMissing(String),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Short machine-readable code for the app.
    pub fn code(&self) -> &'static str {
        match self {
            Error::NeedPassword => "need_password",
            Error::WrongPassword => "wrong_password",
            Error::Unknown => "unknown",
            Error::Unsupported(_) => "unsupported",
            Error::Damaged(_) => "damaged",
            Error::Cancelled => "cancelled",
            Error::PiecesMissing(_) => "pieces_missing",
            Error::Io(e) if e.kind() == io::ErrorKind::StorageFull => "disk_full",
            Error::Io(_) => "io",
            Error::Other(_) => "other",
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Progress + cancel hook. Called from the worker thread.
pub trait Progress: Sync {
    fn update(&self, _done: u64, _total: u64, _current: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

/// A Progress that never reports and can be cancelled by flag.
#[derive(Default)]
pub struct Quiet(pub AtomicBool);
impl Progress for Quiet {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Look at the first bytes of a file and say what it is.
pub fn detect(path: &Path) -> Result<Format> {
    // A piece of a split set: check the set is complete, then look at the joined whole.
    split::pieces(path)?;
    let mut f = split::open(path)?;
    let mut head = vec![0u8; 0x8006];
    let n = read_up_to(&mut f, &mut head)?;
    head.truncate(n);
    let starts = |m: &[u8]| head.len() >= m.len() && &head[..m.len()] == m;

    if purr::is_purr(&head) {
        return Ok(Format::Purr);
    }
    if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") || starts(b"PK\x07\x08") {
        return Ok(Format::Zip);
    }
    if starts(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return Ok(Format::SevenZ);
    }
    if starts(b"Rar!\x1a\x07") {
        return Ok(Format::Rar);
    }
    if starts(b"MSCF\0\0\0\0") {
        return Ok(Format::Cab);
    }
    if head.len() >= 0x8006 && &head[0x8001..0x8006] == b"CD001" {
        return Ok(Format::Iso);
    }
    if head.len() >= 512 && fmt_stream::looks_like_tar(&head[..512]) {
        return Ok(Format::Tar);
    }
    let single = if starts(&[0x1F, 0x8B]) {
        Some(Format::Gz)
    } else if starts(b"BZh") {
        Some(Format::Bz2)
    } else if starts(&[0xFD, b'7', b'z', b'X', b'Z', 0]) {
        Some(Format::Xz)
    } else if starts(&[0x28, 0xB5, 0x2F, 0xFD]) {
        Some(Format::Zst)
    } else {
        None
    };
    if let Some(fmt) = single {
        f.seek(SeekFrom::Start(0))?;
        return Ok(if fmt_stream::inner_is_tar(fmt, f) {
            match fmt {
                Format::Gz => Format::TarGz,
                Format::Bz2 => Format::TarBz2,
                Format::Xz => Format::TarXz,
                _ => Format::TarZst,
            }
        } else {
            fmt
        });
    }
    // Self-extracting or padded ZIPs: look for the end record near the end.
    if fmt_zip::has_end_record(path) {
        return Ok(Format::Zip);
    }
    // A .purr whose first bytes are damaged still ends with its repair data, or at least has its name.
    if purr::recovery::locate_tail(&mut f).is_some() || split::piece_number(path).map(|(b, _)| b).unwrap_or_else(|| path.to_path_buf()).extension().map(|e| e.eq_ignore_ascii_case("purr")).unwrap_or(false) {
        return Ok(Format::Purr);
    }
    Err(Error::Unknown)
}

/// List what is inside.
pub fn list(path: &Path, password: Option<&str>) -> Result<Listing> {
    list_inner(path, password).map_err(|e| match (e, split::info(path)) {
        // A split archive that looks damaged is most often missing its last piece(s).
        (Error::Damaged(_), Some(si)) => Error::PiecesMissing(format!(
            "Purrzip found {} pieces of this split archive, but it isn't complete: the last piece or pieces are probably missing. Put all the pieces in the same folder and try again.",
            si.pieces
        )),
        (e, _) => e,
    })
}

fn list_inner(path: &Path, password: Option<&str>) -> Result<Listing> {
    let format = detect(path)?;
    let mut purr_info = None;
    let (entries, size_unknown) = match format {
        Format::Zip => (fmt_zip::list(path)?, false),
        Format::SevenZ => (fmt_sevenz::list(path, password)?, false),
        Format::Rar if split::is_split(path) => return Err(rar_pieces()),
        Format::Rar => (fmt_rar::list(path, password)?, false),
        Format::Iso => (fmt_iso::list(path)?, false),
        Format::Cab => (fmt_cab::list(path)?, false),
        Format::Purr => {
            let (entries, info) = purr::list(path, password)?;
            purr_info = Some(info);
            (entries, false)
        }
        Format::Gz | Format::Bz2 | Format::Xz | Format::Zst => (fmt_stream::list_single(path, format)?, true),
        _ => (fmt_stream::list_tar(path, format)?, false),
    };
    let total_size = entries.iter().map(|e| e.size).sum();
    let any_encrypted = entries.iter().any(|e| e.encrypted);
    Ok(Listing { format, entries, total_size, any_encrypted, size_unknown, split: split::info(path), purr: purr_info })
}

/// Extract everything (or only `only`, a list of entry indexes) into `dest`.
/// `dest` must already exist. Nothing is ever written outside it.
pub fn extract(
    path: &Path,
    dest: &Path,
    password: Option<&str>,
    only: Option<&[usize]>,
    progress: &dyn Progress,
) -> Result<Report> {
    let listing = list(path, password)?;
    extract_listed(path, &listing, dest, password, only, progress)
}

/// Same as `extract`, reusing a listing the caller already has.
pub fn extract_listed(
    path: &Path,
    listing: &Listing,
    dest: &Path,
    password: Option<&str>,
    only: Option<&[usize]>,
    progress: &dyn Progress,
) -> Result<Report> {
    let format = listing.format;
    let wanted = Wanted::new(only, &listing.entries);
    let total = listing
        .entries
        .iter()
        .filter(|e| wanted.has(e.index))
        .map(|e| e.size)
        .sum::<u64>();
    let mut sink = safe::Sink::new(dest, total, progress)?;
    match format {
        Format::Zip => fmt_zip::extract(path, password, &wanted, &mut sink)?,
        Format::SevenZ => fmt_sevenz::extract(path, password, &wanted, &mut sink)?,
        Format::Rar if split::is_split(path) => return Err(rar_pieces()),
        Format::Rar => fmt_rar::extract(path, password, &wanted, &mut sink)?,
        Format::Iso => fmt_iso::extract(path, &wanted, &mut sink)?,
        Format::Cab => fmt_cab::extract(path, &wanted, &mut sink)?,
        Format::Purr => purr::extract(path, password, &wanted, &mut sink)?,
        Format::Gz | Format::Bz2 | Format::Xz | Format::Zst => {
            if wanted.has(0) {
                fmt_stream::extract_single(path, format, &listing.entries[0], &mut sink)?
            }
        }
        _ => fmt_stream::extract_tar(path, format, &wanted, &mut sink)?,
    }
    Ok(sink.finish())
}

/// RAR cut into plain .001 pieces (rare: WinRAR itself names its pieces name.part1.rar, which opens fine).
fn rar_pieces() -> Error {
    Error::Unsupported("a RAR cut into .001/.002 pieces. Join them into one .rar first, or ask for WinRAR's own name.part1.rar pieces".into())
}

/// Check every file decodes correctly. Writes into a temporary folder that is removed after.
pub fn test(path: &Path, password: Option<&str>, progress: &dyn Progress) -> Result<Report> {
    let dir = temp_folder("test")?;
    let r = extract(path, &dir, password, None, progress);
    let _ = std::fs::remove_dir_all(&dir);
    r
}

/// A new empty folder in the system temp area.
pub fn temp_folder(tag: &str) -> Result<PathBuf> {
    let base = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    for i in 0..1000u32 {
        let p = base.join(format!("purrzip-{}-{}-{}-{}", tag, std::process::id(), nanos, i));
        if std::fs::create_dir(&p).is_ok() {
            return Ok(p);
        }
    }
    Err(Error::Other("Couldn't make a temporary folder.".into()))
}

/// Which entries the caller asked for.
pub struct Wanted {
    all: bool,
    set: Vec<bool>,
}
impl Wanted {
    fn new(only: Option<&[usize]>, entries: &[Entry]) -> Self {
        match only {
            None => Wanted { all: true, set: vec![] },
            Some(ix) => {
                let mut set = vec![false; entries.len()];
                for &i in ix {
                    if i < set.len() {
                        set[i] = true;
                        // Picking a folder picks everything under it.
                        if entries[i].is_dir {
                            let pre = format!("{}/", entries[i].path.trim_end_matches('/'));
                            for e in entries {
                                if e.path.starts_with(&pre) {
                                    set[e.index] = true;
                                }
                            }
                        }
                    }
                }
                Wanted { all: false, set }
            }
        }
    }
    pub fn has(&self, i: usize) -> bool {
        self.all || self.set.get(i).copied().unwrap_or(false)
    }
    /// True when nothing after index `i` is wanted (lets readers stop early).
    pub fn done_after(&self, i: usize) -> bool {
        !self.all && !self.set.iter().skip(i + 1).any(|&b| b)
    }
}

pub(crate) fn read_up_to(r: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// Days since 1970-01-01 to (y, m, d), and back. Proleptic Gregorian.
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
pub(crate) fn unix_from_parts(y: i64, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> i64 {
    days_from_civil(y, mo, d) * 86400 + h as i64 * 3600 + mi as i64 * 60 + s as i64
}
