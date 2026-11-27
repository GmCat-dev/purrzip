use crate::safe::Sink;
use crate::{unix_from_parts, Entry, Error, Result, Wanted};
use crate::split::Source;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use zip::result::ZipError;
use zip::ZipArchive;

pub(crate) fn map_err(e: ZipError) -> Error {
    match e {
        ZipError::InvalidPassword => Error::WrongPassword,
        ZipError::Io(io) => Error::Io(io),
        ZipError::InvalidArchive(m) => Error::Damaged(m.to_string()),
        ZipError::CompressionMethodNotSupported(m) => Error::Unsupported(format!("ZIP compression method {}", m)),
        ZipError::UnsupportedArchive(m) => {
            if m.to_lowercase().contains("password") {
                Error::NeedPassword
            } else {
                Error::Unsupported(m.to_string())
            }
        }
        other => Error::Other(other.to_string()),
    }
}

fn open(path: &Path) -> Result<ZipArchive<BufReader<Source>>> {
    ZipArchive::new(BufReader::new(crate::split::open(path)?)).map_err(map_err)
}

fn to_unix(d: Option<zip::DateTime>) -> Option<i64> {
    let d = d?;
    if d.year() < 1981 {
        return None;
    }
    Some(unix_from_parts(d.year() as i64, d.month() as u32, d.day() as u32, d.hour() as u32, d.minute() as u32, d.second() as u32))
}

pub fn has_end_record(path: &Path) -> bool {
    let Ok(mut f) = crate::split::open(path) else { return false };
    let Ok(len) = f.seek(SeekFrom::End(0)) else { return false };
    let start = len.saturating_sub(66 * 1024 + 22);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return false;
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return false;
    }
    buf.windows(4).any(|w| w == b"PK\x05\x06") && crate::split::open(path).map(|s| ZipArchive::new(BufReader::new(s)).is_ok()).unwrap_or(false)
}

pub fn list(path: &Path) -> Result<Vec<Entry>> {
    let mut z = open(path)?;
    let mut out = Vec::with_capacity(z.len());
    for i in 0..z.len() {
        let f = z.by_index_raw(i).map_err(map_err)?;
        let name = f.name().map(|c| c.into_owned()).unwrap_or_else(|_| String::from_utf8_lossy(f.name_raw()).into_owned());
        out.push(Entry {
            index: i,
            path: name.trim_end_matches('/').to_string(),
            size: f.size(),
            packed: Some(f.compressed_size()),
            is_dir: f.is_dir(),
            encrypted: f.encrypted(),
            modified: to_unix(f.last_modified()),
            special: f.is_symlink(),
        });
    }
    Ok(out)
}

pub fn extract(path: &Path, password: Option<&str>, wanted: &Wanted, sink: &mut Sink) -> Result<()> {
    let entries = list(path)?;
    let mut z = open(path)?;
    for e in &entries {
        if !wanted.has(e.index) {
            continue;
        }
        sink.check()?;
        if e.is_dir {
            sink.dir(e.index, &e.path)?;
            continue;
        }
        if e.special {
            sink.skip(&e.path, "link (Purrzip never creates links from archives)");
            continue;
        }
        let mut f = if e.encrypted {
            let pw = password.ok_or(Error::NeedPassword)?;
            z.by_index_decrypt(e.index, pw.as_bytes()).map_err(map_err)?
        } else {
            z.by_index(e.index).map_err(map_err)?
        };
        match sink.file(e.index, &e.path, &mut f, e.modified) {
            // Old ZipCrypto only notices a wrong password when the checksum fails.
            Err(Error::Damaged(m)) if e.encrypted => {
                return Err(if m.to_lowercase().contains("checksum") || m.to_lowercase().contains("crc") {
                    Error::WrongPassword
                } else {
                    Error::Damaged(m)
                })
            }
            r => r?,
        }
    }
    Ok(())
}
