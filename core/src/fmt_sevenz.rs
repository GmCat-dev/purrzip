use crate::safe::Sink;
use crate::{Entry, Error, Result, Wanted};
use sevenz_rust2::{Archive, ArchiveReader, EntryDecision, Password};
use std::path::Path;

fn pw(p: Option<&str>) -> Password {
    match p {
        Some(s) => Password::from(s),
        None => Password::empty(),
    }
}

pub(crate) fn map_err(e: sevenz_rust2::Error, encrypted: bool) -> Error {
    use sevenz_rust2::Error as E;
    match e {
        E::PasswordRequired => Error::NeedPassword,
        E::MaybeBadPassword(_) => Error::WrongPassword,
        E::ChecksumVerificationFailed | E::NextHeaderCrcMismatch if encrypted => Error::WrongPassword,
        E::UnsupportedCompressionMethod(m) => Error::Unsupported(format!("7z method {}", m)),
        E::Io(io, _) | E::FileOpen(io, _) => {
            if encrypted && io.kind() != std::io::ErrorKind::NotFound {
                Error::WrongPassword
            } else {
                Error::Io(io)
            }
        }
        other => {
            let m = other.to_string();
            if encrypted {
                Error::WrongPassword
            } else {
                Error::Damaged(m)
            }
        }
    }
}

fn nt_to_unix(t: sevenz_rust2::NtTime) -> Option<i64> {
    let st: std::time::SystemTime = t.into();
    st.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

fn open(path: &Path, password: Option<&str>) -> Result<Archive> {
    Archive::read(&mut crate::split::open(path)?, &pw(password)).map_err(|e| map_err(e, password.is_some()))
}

pub fn list(path: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    let a = open(path, password)?;
    Ok(a
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| Entry {
            index: i,
            path: f.name.trim_end_matches('/').to_string(),
            size: f.size,
            packed: None,
            is_dir: f.is_directory,
            encrypted: a.is_file_encrypted(i),
            modified: if f.has_last_modified_date { nt_to_unix(f.last_modified_date) } else { None },
            special: f.is_anti_item,
        })
        .collect())
}

pub fn extract(path: &Path, password: Option<&str>, wanted: &Wanted, sink: &mut Sink) -> Result<()> {
    let archive = open(path, password)?;
    let any_encrypted = (0..archive.files.len()).any(|i| archive.is_file_encrypted(i));
    if any_encrypted && password.is_none() {
        return Err(Error::NeedPassword);
    }
    let names: Vec<String> = archive.files.iter().map(|f| f.name.clone()).collect();
    let mut used = vec![false; names.len()];
    let mut reader = ArchiveReader::from_archive(archive, crate::split::open(path)?, pw(password));
    let threads = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1).min(2);
    reader.set_thread_count(threads);

    let mut next = 0usize;
    let mut failure: Option<Error> = None;
    let res = reader.for_each_entries_with_decision(|entry, data| {
        // Find this entry's index (normally the next one in order).
        let i = if next < names.len() && !used[next] && names[next] == entry.name {
            next
        } else {
            match (0..names.len()).find(|&k| !used[k] && names[k] == entry.name) {
                Some(k) => k,
                None => return Ok(EntryDecision::Continue),
            }
        };
        used[i] = true;
        next = i + 1;
        if !wanted.has(i) {
            return Ok(EntryDecision::Continue);
        }
        let name = entry.name.trim_end_matches('/').to_string();
        let r = if entry.is_anti_item {
            Ok(())
        } else if entry.is_directory {
            sink.dir(i, &name)
        } else {
            let mtime = if entry.has_last_modified_date { nt_to_unix(entry.last_modified_date) } else { None };
            sink.file(i, &name, data, mtime)
        };
        match r {
            Ok(()) if wanted.done_after(i) => Ok(EntryDecision::Stop),
            Ok(()) => Ok(EntryDecision::Continue),
            Err(e) => {
                failure = Some(e);
                Ok(EntryDecision::Stop)
            }
        }
    });
    if let Some(e) = failure {
        return Err(match e {
            Error::Damaged(_) if any_encrypted => Error::WrongPassword,
            other => other,
        });
    }
    res.map_err(|e| map_err(e, any_encrypted))
}
