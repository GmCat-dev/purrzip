//! Making .zip and .7z archives.

use crate::{civil_from_days, Error, Progress, Result};
use sevenz_rust2::encoder_options::{AesEncoderOptions, Lzma2Options};
use sevenz_rust2::{ArchiveEntry, ArchiveWriter, SourceReader};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use zip::write::SimpleFileOptions;
use zip::{AesMode, CompressionMethod, ZipWriter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum CreateFormat {
    Zip,
    SevenZ,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum Level {
    /// Just pack, no squeezing (fastest).
    Store,
    Fast,
    Normal,
    Max,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CreateOptions {
    pub format: CreateFormat,
    pub level: Level,
    pub password: Option<String>,
    /// 7z only: also hide the file names behind the password.
    #[serde(default)]
    pub hide_names: bool,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct CreateReport {
    pub files: u64,
    pub folders: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub skipped: Vec<(String, String)>,
}

struct Item {
    src: PathBuf,
    name: String,
    is_dir: bool,
    size: u64,
}

/// Files that are already squeezed: storing them is as small and much faster.
fn already_packed(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "avif" | "heic" | "heif" | "jxl" | "mp4" | "m4v" | "mov" | "mkv"
            | "webm" | "avi" | "mp3" | "m4a" | "aac" | "ogg" | "opus" | "flac" | "zip" | "7z" | "rar" | "gz" | "tgz"
            | "bz2" | "xz" | "zst" | "apk" | "jar" | "docx" | "xlsx" | "pptx" | "odt" | "epub" | "woff" | "woff2"
    )
}

fn component_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "files".into())
}

fn gather(sources: &[PathBuf], skipped: &mut Vec<(String, String)>) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    let mut tops = HashSet::new();
    for src in sources {
        let meta = fs::symlink_metadata(src)?;
        let mut top = component_name(src);
        let mut n = 2;
        while !tops.insert(top.to_lowercase()) {
            top = format!("{} ({})", component_name(src), n);
            n += 1;
        }
        if meta.file_type().is_symlink() {
            skipped.push((top, "link (not followed)".into()));
        } else if meta.is_file() {
            items.push(Item { src: src.clone(), name: top, is_dir: false, size: meta.len() });
        } else if meta.is_dir() {
            for e in walkdir::WalkDir::new(src).follow_links(false).sort_by_file_name() {
                let e = match e {
                    Ok(e) => e,
                    Err(err) => {
                        skipped.push((err.path().map(|p| p.display().to_string()).unwrap_or_default(), "couldn't read".into()));
                        continue;
                    }
                };
                let rel = e.path().strip_prefix(src).unwrap_or(e.path());
                let mut name = top.clone();
                for c in rel.components() {
                    name.push('/');
                    name.push_str(&c.as_os_str().to_string_lossy());
                }
                let ft = e.file_type();
                if ft.is_symlink() {
                    skipped.push((name, "link (not followed)".into()));
                } else if ft.is_dir() {
                    items.push(Item { src: e.path().to_path_buf(), name, is_dir: true, size: 0 });
                } else if ft.is_file() {
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    items.push(Item { src: e.path().to_path_buf(), name, is_dir: false, size });
                }
            }
        }
    }
    Ok(items)
}

/// Reads a file, counting bytes for progress and stopping when cancelled. Opens lazily.
struct Counted<'a> {
    path: PathBuf,
    file: Option<File>,
    name: String,
    done: &'a AtomicU64,
    last: u64,
    total: u64,
    progress: &'a dyn Progress,
}
impl Read for Counted<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.progress.cancelled() {
            return Err(io::Error::other("cancelled by user"));
        }
        if self.file.is_none() {
            self.file = Some(File::open(&self.path)?);
            self.progress.update(self.done.load(Ordering::Relaxed), self.total, &self.name);
        }
        let n = self.file.as_mut().unwrap().read(buf)?;
        let d = self.done.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
        if d - self.last >= 1 << 20 {
            self.last = d;
            self.progress.update(d, self.total, &self.name);
        }
        Ok(n)
    }
}

/// Pack `sources` (files and/or folders) into `out`. Writes to `out.part` first and
/// renames at the end, so a stopped or failed run never leaves a half archive behind.
pub fn create(sources: &[PathBuf], out: &Path, opts: &CreateOptions, progress: &dyn Progress) -> Result<CreateReport> {
    if sources.is_empty() {
        return Err(Error::Other("Pick at least one file or folder.".into()));
    }
    let mut report = CreateReport::default();
    let items = gather(sources, &mut report.skipped)?;
    let total: u64 = items.iter().map(|i| i.size).sum();
    let part = PathBuf::from(format!("{}.part", out.display()));
    let result = match opts.format {
        CreateFormat::Zip => write_zip(&items, &part, opts, total, progress),
        CreateFormat::SevenZ => write_7z(&items, &part, opts, total, progress),
    };
    if let Err(e) = result {
        let _ = fs::remove_file(&part);
        return Err(if progress.cancelled() { Error::Cancelled } else { e });
    }
    if out.exists() {
        let _ = fs::remove_file(&part);
        return Err(Error::Other("A file with that name appeared while packing; nothing was replaced.".into()));
    }
    fs::rename(&part, out)?;
    report.files = items.iter().filter(|i| !i.is_dir).count() as u64;
    report.folders = items.iter().filter(|i| i.is_dir).count() as u64;
    report.bytes_in = total;
    report.bytes_out = fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    progress.update(total, total, "");
    Ok(report)
}

fn zip_time(path: &Path) -> Option<zip::DateTime> {
    let t = fs::metadata(path).ok()?.modified().ok()?;
    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let (y, m, d) = civil_from_days(secs.div_euclid(86400));
    let s = secs.rem_euclid(86400);
    zip::DateTime::from_date_and_time(y as u16, m as u8, d as u8, (s / 3600) as u8, (s / 60 % 60) as u8, (s % 60) as u8).ok()
}

fn write_zip(items: &[Item], part: &Path, opts: &CreateOptions, total: u64, progress: &dyn Progress) -> Result<()> {
    let file = fs::OpenOptions::new().write(true).create_new(true).open(part)?;
    let mut z = ZipWriter::new(BufWriter::with_capacity(1 << 20, file));
    let done = AtomicU64::new(0);
    let level = match opts.level {
        Level::Store => None,
        Level::Fast => Some(1),
        Level::Normal => Some(6),
        Level::Max => Some(9),
    };
    let zerr = crate::fmt_zip::map_err;
    for it in items {
        if progress.cancelled() {
            return Err(Error::Cancelled);
        }
        let mut o: SimpleFileOptions = SimpleFileOptions::default();
        if let Some(t) = zip_time(&it.src) {
            o = o.last_modified_time(t);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(m) = fs::metadata(&it.src) {
                o = o.unix_permissions(m.permissions().mode() & 0o777);
            }
        }
        if it.is_dir {
            z.add_directory(format!("{}/", it.name), o).map_err(zerr)?;
            continue;
        }
        o = match level {
            Some(l) if !already_packed(&it.name) && it.size > 0 => o.compression_method(CompressionMethod::Deflated).compression_level(Some(l)),
            _ => o.compression_method(CompressionMethod::Stored),
        };
        o = o.large_file(it.size >= 0xFFFF_0000);
        let pw = opts.password.as_deref().filter(|p| !p.is_empty());
        let o = match pw {
            Some(p) => o.with_aes_encryption(AesMode::Aes256, p),
            None => o,
        };
        z.start_file(it.name.clone(), o).map_err(zerr)?;
        let mut r = Counted { path: it.src.clone(), file: None, name: it.name.clone(), done: &done, last: 0, total, progress };
        io::copy(&mut r, &mut z).map_err(|e| if progress.cancelled() { Error::Cancelled } else { Error::Io(e) })?;
    }
    let mut w = z.finish().map_err(zerr)?;
    w.flush()?;
    w.get_ref().sync_all()?;
    Ok(())
}

fn write_7z(items: &[Item], part: &Path, opts: &CreateOptions, total: u64, progress: &dyn Progress) -> Result<()> {
    let file = fs::OpenOptions::new().write(true).create_new(true).open(part)?;
    let mut w = ArchiveWriter::new(BufWriter::with_capacity(1 << 20, file)).map_err(|e| crate::fmt_sevenz::map_err(e, false))?;
    let level = match opts.level {
        Level::Store => 0,
        Level::Fast => 1,
        Level::Normal => 5,
        Level::Max => 9,
    };
    // Two worker threads with small chunks keeps memory near 200 MB, which phones can afford.
    let threads = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1).min(2);
    let lz = || {
        if threads > 1 && level > 0 && level < 9 {
            Lzma2Options::from_level_mt(level, threads, 16 << 20)
        } else {
            Lzma2Options::from_level(level)
        }
    };
    let pw = opts.password.as_deref().filter(|p| !p.is_empty());
    let methods = |packed: bool| -> Vec<sevenz_rust2::EncoderConfiguration> {
        let inner: sevenz_rust2::EncoderConfiguration =
            if packed || level == 0 { sevenz_rust2::EncoderMethod::COPY.into() } else { lz().into() };
        match pw {
            Some(p) => vec![AesEncoderOptions::new(p.into()).into(), inner],
            None => vec![inner],
        }
    };
    if pw.is_some() {
        w.set_encrypt_header(opts.hide_names);
    }
    w.set_content_methods(methods(false));
    let serr = |e| if progress.cancelled() { Error::Cancelled } else { crate::fmt_sevenz::map_err(e, false) };
    for it in items.iter().filter(|i| i.is_dir) {
        w.push_archive_entry::<&[u8]>(ArchiveEntry::from_path(&it.src, it.name.clone()), None).map_err(serr)?;
    }
    let done = AtomicU64::new(0);
    let files: Vec<&Item> = items.iter().filter(|i| !i.is_dir).collect();
    // Empty files carry no data stream.
    for it in files.iter().filter(|i| i.size == 0) {
        let mut e = ArchiveEntry::from_path(&it.src, it.name.clone());
        e.has_stream = false;
        w.push_archive_entry::<&[u8]>(e, None).map_err(serr)?;
    }
    // Text, documents etc. get squeezed together; photos/videos/zips are stored as they are.
    for packed in [false, true] {
        let group: Vec<&&Item> = files.iter().filter(|i| i.size > 0 && already_packed(&i.name) == packed).collect();
        if group.is_empty() {
            continue;
        }
        w.set_content_methods(methods(packed));
        let entries: Vec<ArchiveEntry> = group.iter().map(|it| ArchiveEntry::from_path(&it.src, it.name.clone())).collect();
        let readers: Vec<SourceReader<Counted>> = group
            .iter()
            .map(|it| SourceReader::new(Counted { path: it.src.clone(), file: None, name: it.name.clone(), done: &done, last: 0, total, progress }))
            .collect();
        w.push_archive_entries(entries, readers).map_err(serr)?;
    }
    let mut out = w.finish().map_err(|e| if progress.cancelled() { Error::Cancelled } else { Error::Io(e) })?;
    out.flush()?;
    out.get_ref().sync_all()?;
    Ok(())
}
