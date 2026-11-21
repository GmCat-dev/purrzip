//! TAR (plain or inside gz/bz2/xz/zst) and single compressed files.

use crate::safe::Sink;
use crate::{read_up_to, Entry, Error, Format, Result, Wanted};
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

fn decoder(fmt: Format, f: File) -> io::Result<Box<dyn Read>> {
    let b = BufReader::with_capacity(256 * 1024, f);
    Ok(match fmt {
        Format::Gz | Format::TarGz => Box::new(flate2::read::MultiGzDecoder::new(b)),
        Format::Bz2 | Format::TarBz2 => Box::new(bzip2::read::MultiBzDecoder::new(b)),
        Format::Xz | Format::TarXz => Box::new(lzma_rust2::XzReader::new(b, true)),
        Format::Zst | Format::TarZst => Box::new(zstd::stream::read::Decoder::with_buffer(b)?),
        _ => Box::new(b),
    })
}

pub fn looks_like_tar(h: &[u8]) -> bool {
    if h.len() < 512 {
        return false;
    }
    if &h[257..262] == b"ustar" {
        return true;
    }
    // Old-style tar: check the header checksum.
    let field = &h[148..156];
    let digits: String = field.iter().take_while(|&&c| c != 0 && c != b' ').map(|&c| c as char).collect();
    let digits = digits.trim();
    if digits.is_empty() || !digits.chars().all(|c| ('0'..='7').contains(&c)) {
        return false;
    }
    let Ok(want) = u32::from_str_radix(digits, 8) else { return false };
    let sum: u32 = h[..512].iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u32 }).sum();
    sum == want && h[0] != 0
}

pub fn inner_is_tar(fmt: Format, f: File) -> bool {
    let Ok(mut d) = decoder(fmt, f) else { return false };
    let mut buf = [0u8; 512];
    matches!(read_up_to(&mut d, &mut buf), Ok(512)) && looks_like_tar(&buf)
}

fn map_err(e: io::Error) -> Error {
    match e.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput | io::ErrorKind::UnexpectedEof => Error::Damaged(e.to_string()),
        _ => Error::Io(e),
    }
}

pub fn list_tar(path: &Path, fmt: Format) -> Result<Vec<Entry>> {
    let mut ar = tar::Archive::new(decoder(fmt, File::open(path)?)?);
    let mut out = Vec::new();
    for e in ar.entries().map_err(map_err)? {
        let e = e.map_err(map_err)?;
        let h = e.header();
        let kind = h.entry_type();
        let name = e.path().map_err(map_err)?.to_string_lossy().replace('\\', "/");
        let name = name.trim_end_matches('/').to_string();
        if name.is_empty() || name == "." {
            continue;
        }
        out.push(Entry {
            index: out.len(),
            path: name,
            size: if kind.is_file() { e.size() } else { 0 },
            packed: None,
            is_dir: kind.is_dir(),
            encrypted: false,
            modified: h.mtime().ok().map(|t| t as i64),
            special: !(kind.is_file() || kind.is_dir()),
        });
    }
    Ok(out)
}

pub fn extract_tar(path: &Path, fmt: Format, wanted: &Wanted, sink: &mut Sink) -> Result<()> {
    let mut ar = tar::Archive::new(decoder(fmt, File::open(path)?)?);
    let mut i = 0usize;
    for e in ar.entries().map_err(map_err)? {
        sink.check()?;
        let mut e = e.map_err(map_err)?;
        let kind = e.header().entry_type();
        let name = e.path().map_err(map_err)?.to_string_lossy().replace('\\', "/");
        let name = name.trim_end_matches('/').to_string();
        if name.is_empty() || name == "." {
            continue;
        }
        let idx = i;
        i += 1;
        if !wanted.has(idx) {
            continue;
        }
        let mtime = e.header().mtime().ok().map(|t| t as i64);
        if kind.is_dir() {
            sink.dir(idx, &name)?;
        } else if kind.is_file() {
            sink.file(idx, &name, &mut e, mtime)?;
        } else {
            sink.skip(&name, "link or special file (never created from archives)");
        }
        if wanted.done_after(idx) {
            break;
        }
    }
    Ok(())
}

/// Name for a single compressed file: the name stored in a .gz, or the file name minus its ending.
fn single_name(path: &Path, fmt: Format) -> String {
    if fmt == Format::Gz {
        if let Some(n) = gz_stored_name(path) {
            return n;
        }
    }
    let file = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = file.to_lowercase();
    for ext in [".gz", ".gzip", ".bz2", ".bzip2", ".xz", ".zst", ".zstd"] {
        if lower.ends_with(ext) && file.len() > ext.len() {
            return file[..file.len() - ext.len()].to_string();
        }
    }
    format!("{} (opened)", file)
}

fn gz_stored_name(path: &Path) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let mut h = vec![0u8; 4096];
    let n = read_up_to(&mut f, &mut h).ok()?;
    h.truncate(n);
    if h.len() < 10 || h[0] != 0x1f || h[1] != 0x8b {
        return None;
    }
    let flg = h[3];
    if flg & 8 == 0 {
        return None;
    }
    let mut p = 10usize;
    if flg & 4 != 0 {
        let xlen = u16::from_le_bytes([*h.get(p)?, *h.get(p + 1)?]) as usize;
        p += 2 + xlen;
    }
    let end = h.get(p..)?.iter().position(|&b| b == 0)? + p;
    let raw: String = h[p..end].iter().map(|&b| b as char).collect(); // stored as Latin-1
    let base = raw.replace('\\', "/").rsplit('/').next().unwrap_or("").to_string();
    if base.is_empty() { None } else { Some(base) }
}

pub fn list_single(path: &Path, fmt: Format) -> Result<Vec<Entry>> {
    let mut size = 0u64;
    if fmt == Format::Gz {
        // Last 4 bytes hold the size modulo 4 GB: good enough as a hint.
        let mut f = File::open(path)?;
        if f.seek(SeekFrom::End(-4)).is_ok() {
            let mut b = [0u8; 4];
            if f.read_exact(&mut b).is_ok() {
                size = u32::from_le_bytes(b) as u64;
            }
        }
    }
    let modified = std::fs::metadata(path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
    Ok(vec![Entry {
        index: 0,
        path: single_name(path, fmt),
        size,
        packed: std::fs::metadata(path).ok().map(|m| m.len()),
        is_dir: false,
        encrypted: false,
        modified,
        special: false,
    }])
}

pub fn extract_single(path: &Path, fmt: Format, entry: &Entry, sink: &mut Sink) -> Result<()> {
    let mut d = decoder(fmt, File::open(path)?)?;
    sink.file(0, &entry.path, &mut d, entry.modified)
}
