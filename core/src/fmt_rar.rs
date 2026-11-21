//! RAR reading through the UnRAR library (extraction only; RAR can't be created).
//! UnRAR source code may be used in any software to handle RAR archives without
//! limitations free of charge, but cannot be used to develop RAR (WinRAR)
//! compatible archiver and to re-create RAR compression algorithm, which is
//! proprietary. Distribution of modified UnRAR source code in separate form or as
//! a part of other software is permitted, provided that full text of this
//! paragraph, starting from "UnRAR source code" words, is included in license, or
//! in documentation if license is not available, and in source code comments of
//! resulting package.

use crate::safe::Sink;
use crate::{unix_from_parts, Entry, Error, Result, Wanted};
use std::path::Path;
use unrar::error::{Code, UnrarError};
use unrar::Archive;

fn map_err(e: UnrarError, encrypted: bool) -> Error {
    match e.code {
        Code::MissingPassword => Error::NeedPassword,
        Code::BadPassword => Error::WrongPassword,
        Code::BadData if encrypted => Error::WrongPassword,
        Code::BadData | Code::BadArchive | Code::UnknownFormat => Error::Damaged(format!("{:?}", e.code)),
        Code::EOpen => Error::Other("Couldn't open a part of this archive (is a .part file missing?).".into()),
        Code::NoMemory => Error::Other("Not enough memory.".into()),
        Code::ECreate | Code::EWrite => Error::Other("Couldn't write a file (is the disk full?).".into()),
        other => Error::Other(format!("RAR error: {:?}", other)),
    }
}

fn dos_to_unix(t: u32) -> Option<i64> {
    if t == 0 {
        return None;
    }
    let date = t >> 16;
    let time = t & 0xffff;
    let y = 1980 + (date >> 9) as i64;
    let mo = (date >> 5) & 15;
    let d = date & 31;
    if mo == 0 || d == 0 {
        return None;
    }
    Some(unix_from_parts(y, mo, d, time >> 11, (time >> 5) & 63, (time & 31) * 2))
}

fn path_of(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_string()
}

pub fn list(path: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    let first = Archive::new(&path).as_first_part();
    let first = first.filename().to_path_buf();
    let a = match password {
        Some(p) => Archive::with_password(&first, p),
        None => Archive::new(&first),
    };
    let open = a.open_for_listing().map_err(|e| map_err(e, password.is_some()))?;
    let mut out = Vec::new();
    for h in open {
        let h = h.map_err(|e| map_err(e, password.is_some()))?;
        if h.is_split_before() {
            continue; // continuation of a file from the previous part
        }
        out.push(Entry {
            index: out.len(),
            path: path_of(&h.filename),
            size: h.unpacked_size,
            packed: None,
            is_dir: h.is_directory(),
            encrypted: h.is_encrypted(),
            modified: dos_to_unix(h.file_time),
            special: false,
        });
    }
    Ok(out)
}

pub fn extract(path: &Path, password: Option<&str>, wanted: &Wanted, sink: &mut Sink) -> Result<()> {
    let first = Archive::new(&path).as_first_part();
    let first = first.filename().to_path_buf();
    let a = match password {
        Some(p) => Archive::with_password(&first, p),
        None => Archive::new(&first),
    };
    let mut open = a.open_for_processing().map_err(|e| map_err(e, password.is_some()))?;
    let mut i = 0usize;
    loop {
        sink.check()?;
        let Some(h) = open.read_header().map_err(|e| map_err(e, password.is_some()))? else { break };
        let (split_before, encrypted, is_dir, fname) = {
            let e = h.entry();
            (e.is_split_before(), e.is_encrypted(), e.is_directory(), e.filename.clone())
        };
        if split_before {
            open = h.skip().map_err(|e| map_err(e, password.is_some()))?;
            continue;
        }
        let idx = i;
        i += 1;
        let name = path_of(&fname);
        if !wanted.has(idx) {
            open = h.skip().map_err(|e| map_err(e, encrypted))?;
            continue;
        }
        if encrypted && password.is_none() {
            return Err(Error::NeedPassword);
        }
        if is_dir {
            sink.dir(idx, &name)?;
            open = h.skip().map_err(|e| map_err(e, false))?;
            continue;
        }
        match sink.reserve(&name)? {
            None => open = h.skip().map_err(|e| map_err(e, false))?,
            Some((full, rel)) => {
                open = h.extract_to(&full).map_err(|e| {
                    let _ = std::fs::remove_file(&full);
                    map_err(e, encrypted)
                })?;
                // Never keep a link made from an archive.
                match std::fs::symlink_metadata(&full) {
                    Ok(m) if m.file_type().is_symlink() => {
                        let _ = std::fs::remove_file(&full);
                        sink.skip(&name, "link (Purrzip never creates links from archives)");
                    }
                    Ok(m) if m.is_file() => sink.wrote(idx, rel, m.len(), &name),
                    _ => sink.skip(&name, "not written"),
                }
            }
        }
        if wanted.done_after(idx) {
            break;
        }
    }
    Ok(())
}
