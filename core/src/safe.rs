//! Safe writing of extracted files.
//!
//! Every name from an archive is cleaned before use: no `..`, no absolute paths,
//! no drive letters, no characters Windows can't store, no reserved device names.
//! Files are created with "create new" so nothing already on disk is overwritten,
//! links from archives are never created, and each parent folder is checked to be
//! inside the destination before writing.

use crate::{Error, Progress, Report, Result};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Clean an archive path. Returns None when the name tries to climb out (`..`).
/// The bool says whether anything had to change.
pub fn clean_rel(name: &str) -> Option<(Vec<String>, bool)> {
    let unified = name.replace('\\', "/");
    let mut out = Vec::new();
    let mut changed = unified != name;
    for part in unified.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        let mut c: String = part
            .chars()
            .map(|ch| match ch {
                '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
                ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => '_',
                ch => ch,
            })
            .collect();
        let trimmed = c.trim_end_matches([' ', '.']).to_string();
        c = if trimmed.is_empty() { "_".to_string() } else { trimmed };
        let stem = c.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
        if RESERVED.contains(&stem.as_str()) {
            c = format!("_{}", c);
        }
        if c.len() > 240 {
            c = shorten(&c, 240);
        }
        if c != part {
            changed = true;
        }
        out.push(c);
    }
    if out.is_empty() {
        out.push("unnamed".to_string());
        changed = true;
    }
    Some((out, changed))
}

fn shorten(name: &str, max: usize) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if name.len() - i <= 16 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut cut = max.saturating_sub(ext.len());
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{}", &stem[..cut.min(stem.len())], ext)
}

fn with_number(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({}){}", &name[..i], n, &name[i..]),
        _ => format!("{} ({})", name, n),
    }
}

pub struct Sink<'a> {
    root: PathBuf,
    total: u64,
    done: u64,
    last_tick: u64,
    progress: &'a dyn Progress,
    taken: HashSet<String>,
    report: Report,
}

impl<'a> Sink<'a> {
    pub fn new(root: &Path, total: u64, progress: &'a dyn Progress) -> Result<Self> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(Error::Other("The destination isn't a folder.".into()));
        }
        Ok(Sink { root, total, done: 0, last_tick: 0, progress, taken: HashSet::new(), report: Report::default() })
    }

    pub fn check(&self) -> Result<()> {
        if self.progress.cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    pub fn skip(&mut self, name: &str, why: &str) {
        self.report.skipped.push((name.to_string(), why.to_string()));
    }

    fn tick(&mut self, name: &str, force: bool) {
        if force || self.done - self.last_tick >= 1 << 20 {
            self.last_tick = self.done;
            self.progress.update(self.done, self.total.max(self.done), name);
        }
    }

    /// Make sure `dir` (absolute) exists and really sits inside the root.
    fn ensure_dir(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)?;
        let real = fs::canonicalize(dir)?;
        if !real.starts_with(&self.root) {
            return Err(Error::Other("Refused to write outside the chosen folder.".into()));
        }
        Ok(())
    }

    pub fn dir(&mut self, index: usize, name: &str) -> Result<()> {
        self.check()?;
        let Some((parts, _)) = clean_rel(name) else {
            self.skip(name, "unsafe name (tries to leave the folder)");
            return Ok(());
        };
        let p = parts.iter().fold(self.root.clone(), |p, c| p.join(c));
        if p.exists() && !p.is_dir() {
            self.skip(name, "a file with that name is already there");
            return Ok(());
        }
        self.ensure_dir(&p)?;
        self.report.folders += 1;
        self.report.written.push((index, parts.join("/")));
        Ok(())
    }

    /// Pick the final path for a file: cleaned, inside the root, not already used.
    /// Returns (absolute path, relative path) or None if the name was refused.
    pub fn reserve(&mut self, name: &str) -> Result<Option<(PathBuf, String)>> {
        self.check()?;
        let Some((mut parts, changed)) = clean_rel(name) else {
            self.skip(name, "unsafe name (tries to leave the folder)");
            return Ok(None);
        };
        let file_name = parts.pop().unwrap();
        let parent = parts.iter().fold(self.root.clone(), |p, c| p.join(c));
        if parent.exists() && !parent.is_dir() {
            self.skip(name, "its folder name is already used by a file");
            return Ok(None);
        }
        self.ensure_dir(&parent)?;
        let rel_parent = parts.join("/");
        let mut candidate = file_name.clone();
        let mut n = 1;
        loop {
            let rel = if rel_parent.is_empty() { candidate.clone() } else { format!("{}/{}", rel_parent, candidate) };
            let key = rel.to_lowercase();
            let full = parent.join(&candidate);
            if !self.taken.contains(&key) && fs::symlink_metadata(&full).is_err() {
                self.taken.insert(key);
                if changed || n > 1 {
                    self.report.renamed.push((name.to_string(), rel.clone()));
                }
                return Ok(Some((full, rel)));
            }
            n += 1;
            if n > 9999 {
                self.skip(name, "too many files with the same name");
                return Ok(None);
            }
            candidate = with_number(&file_name, n);
        }
    }

    /// Write one file from a reader.
    pub fn file(&mut self, index: usize, name: &str, reader: &mut dyn Read, modified: Option<i64>) -> Result<()> {
        let Some((full, rel)) = self.reserve(name)? else {
            return Ok(());
        };
        self.tick(name, true);
        let out = OpenOptions::new().write(true).create_new(true).open(&full)?;
        match self.copy(name, reader, out, modified) {
            Ok(n) => {
                self.report.files += 1;
                self.report.bytes += n;
                self.report.written.push((index, rel));
                Ok(())
            }
            Err(e) => {
                let _ = fs::remove_file(&full);
                Err(e)
            }
        }
    }

    fn copy(&mut self, name: &str, reader: &mut dyn Read, mut out: File, modified: Option<i64>) -> Result<u64> {
        let mut buf = vec![0u8; 256 * 1024];
        let mut n = 0u64;
        loop {
            self.check()?;
            let k = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => k,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(map_read_err(e)),
            };
            out.write_all(&buf[..k])?;
            n += k as u64;
            self.done += k as u64;
            self.tick(name, false);
        }
        out.flush()?;
        if let Some(t) = modified {
            set_mtime(&out, t);
        }
        Ok(n)
    }

    /// For readers that write files themselves (RAR): record the result.
    pub fn wrote(&mut self, index: usize, rel: String, bytes: u64, name: &str) {
        self.report.files += 1;
        self.report.bytes += bytes;
        self.done += bytes;
        self.report.written.push((index, rel));
        self.tick(name, true);
    }

    pub fn finish(mut self) -> Report {
        self.progress.update(self.done, self.total.max(self.done), "");
        std::mem::take(&mut self.report)
    }
}

pub(crate) fn set_mtime(f: &File, unix: i64) {
    if unix > 0 {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(unix as u64);
        let _ = f.set_modified(t);
    }
}

/// Decoders report bad data in many ways; turn the common ones into "damaged".
fn map_read_err(e: io::Error) -> Error {
    match e.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => Error::Damaged(e.to_string()),
        _ => {
            let msg = e.to_string().to_lowercase();
            if msg.contains("password") {
                Error::WrongPassword
            } else if msg.contains("checksum") || msg.contains("crc") || msg.contains("corrupt") || msg.contains("invalid") {
                Error::Damaged(e.to_string())
            } else {
                Error::Io(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleaning() {
        assert!(clean_rel("../evil").is_none());
        assert!(clean_rel("a/../../evil").is_none());
        assert_eq!(clean_rel("/etc/passwd").unwrap().0, vec!["etc", "passwd"]);
        assert_eq!(clean_rel("C:\\Windows\\x.dll").unwrap().0, vec!["C_", "Windows", "x.dll"]);
        assert_eq!(clean_rel("con.txt").unwrap().0, vec!["_con.txt"]);
        assert_eq!(clean_rel("a?b*.txt ").unwrap().0, vec!["a_b_.txt"]);
        assert_eq!(clean_rel("ok/fine.txt").unwrap(), (vec!["ok".to_string(), "fine.txt".to_string()], false));
        assert_eq!(with_number("cat.png", 2), "cat (2).png");
    }
}
