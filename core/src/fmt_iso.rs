//! Minimal ISO 9660 reader (with Joliet long names). Read-only.

use crate::safe::Sink;
use crate::{read_up_to, unix_from_parts, Entry, Error, Result, Wanted};
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const SECTOR: u64 = 2048;

struct Rec {
    path: String,
    extents: Vec<(u32, u32)>,
    is_dir: bool,
    mtime: Option<i64>,
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn root_of(f: &mut File) -> Result<((u32, u32), bool)> {
    let mut primary = None;
    let mut joliet = None;
    for s in 16..80u64 {
        let mut d = [0u8; 2048];
        f.seek(SeekFrom::Start(s * SECTOR))?;
        if read_up_to(f, &mut d)? < 2048 || &d[1..6] != b"CD001" {
            break;
        }
        let root = (le32(&d[156 + 2..]), le32(&d[156 + 10..]));
        match d[0] {
            1 if primary.is_none() => primary = Some(root),
            2 if d[88] == b'%' && d[89] == b'/' && matches!(d[90], b'@' | b'C' | b'E') => joliet = Some(root),
            255 => break,
            _ => {}
        }
    }
    match (joliet, primary) {
        (Some(j), _) => Ok((j, true)),
        (None, Some(p)) => Ok((p, false)),
        _ => Err(Error::Damaged("no ISO volume found".into())),
    }
}

fn decode_name(raw: &[u8], joliet: bool) -> String {
    let mut s = if joliet {
        let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(raw).into_owned()
    };
    if let Some(i) = s.rfind(';') {
        if s[i + 1..].chars().all(|c| c.is_ascii_digit()) {
            s.truncate(i);
        }
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

fn walk(path: &Path) -> Result<(File, Vec<Rec>)> {
    let mut f = File::open(path)?;
    let (root, joliet) = root_of(&mut f)?;
    let mut out: Vec<Rec> = Vec::new();
    let mut seen = HashSet::new();
    // Stack of (dir extent, dir length, path prefix, depth). Children are pushed in reverse to keep order.
    let mut stack = vec![(root.0, root.1, String::new(), 0u32)];
    while let Some((lba, len, prefix, depth)) = stack.pop() {
        if depth > 64 || !seen.insert(lba) || out.len() > 2_000_000 {
            continue;
        }
        let len = len.min(32 << 20) as usize;
        let mut buf = vec![0u8; len];
        f.seek(SeekFrom::Start(lba as u64 * SECTOR))?;
        let n = read_up_to(&mut f, &mut buf)?;
        buf.truncate(n);
        let mut pos = 0usize;
        let mut subdirs = Vec::new();
        let mut pending_multi: Option<usize> = None;
        while pos < buf.len() {
            let rl = buf[pos] as usize;
            if rl == 0 {
                pos = (pos / SECTOR as usize + 1) * SECTOR as usize;
                continue;
            }
            if rl < 34 || pos + rl > buf.len() {
                break;
            }
            let r = &buf[pos..pos + rl];
            pos += rl;
            let nl = r[32] as usize;
            if 33 + nl > rl {
                continue;
            }
            let raw = &r[33..33 + nl];
            if raw == [0] || raw == [1] {
                continue;
            }
            let flags = r[25];
            let extent = (le32(&r[2..]), le32(&r[10..]));
            let name = decode_name(raw, joliet);
            if name.is_empty() {
                continue;
            }
            let full = if prefix.is_empty() { name.clone() } else { format!("{}/{}", prefix, name) };
            let is_dir = flags & 2 != 0;
            // Multi-extent files: further parts follow with the same name.
            if let Some(k) = pending_multi {
                if out[k].path == full && !is_dir {
                    out[k].extents.push(extent);
                    pending_multi = if flags & 0x80 != 0 { Some(k) } else { None };
                    continue;
                }
            }
            let d = &r[18..25];
            let mtime = if d[1] >= 1 && d[1] <= 12 && d[2] >= 1 {
                let off = (d[6] as i8) as i64 * 15 * 60;
                Some(unix_from_parts(1900 + d[0] as i64, d[1] as u32, d[2] as u32, d[3] as u32, d[4] as u32, d[5] as u32) - off)
            } else {
                None
            };
            out.push(Rec { path: full.clone(), extents: vec![extent], is_dir, mtime });
            if is_dir {
                subdirs.push((extent.0, extent.1, full));
            } else if flags & 0x80 != 0 {
                pending_multi = Some(out.len() - 1);
            }
        }
        for (l, n, p) in subdirs.into_iter().rev() {
            stack.push((l, n, p, depth + 1));
        }
    }
    // Order: a folder's own record first, then its contents (sort by path keeps that).
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((f, out))
}

pub fn list(path: &Path) -> Result<Vec<Entry>> {
    let (_, recs) = walk(path)?;
    Ok(recs
        .iter()
        .enumerate()
        .map(|(i, r)| Entry {
            index: i,
            path: r.path.clone(),
            size: if r.is_dir { 0 } else { r.extents.iter().map(|e| e.1 as u64).sum() },
            packed: None,
            is_dir: r.is_dir,
            encrypted: false,
            modified: r.mtime,
            special: false,
        })
        .collect())
}

struct Extents<'a> {
    f: &'a mut File,
    list: Vec<(u32, u32)>,
    cur: usize,
    left: u64,
}
impl Read for Extents<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.left == 0 {
            if self.cur >= self.list.len() {
                return Ok(0);
            }
            let (lba, len) = self.list[self.cur];
            self.cur += 1;
            self.f.seek(SeekFrom::Start(lba as u64 * SECTOR))?;
            self.left = len as u64;
        }
        let want = buf.len().min(self.left as usize);
        let n = self.f.read(&mut buf[..want])?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "image is cut short"));
        }
        self.left -= n as u64;
        Ok(n)
    }
}

pub fn extract(path: &Path, wanted: &Wanted, sink: &mut Sink) -> Result<()> {
    let (mut f, recs) = walk(path)?;
    for (i, r) in recs.iter().enumerate() {
        if !wanted.has(i) {
            continue;
        }
        if r.is_dir {
            sink.dir(i, &r.path)?;
        } else {
            let mut rd = Extents { f: &mut f, list: r.extents.clone(), cur: 0, left: 0 };
            sink.file(i, &r.path, &mut rd, r.mtime)?;
        }
    }
    Ok(())
}
