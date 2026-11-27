//! Purrzip app service: the commands the app screen calls, shared by the Tauri app
//! (desktop + Android) and the local dev server used for browser tests.
//! Every command takes JSON and returns JSON, so the screen code is the same everywhere.

use purrzip_core::{self as core, CreateOptions, Listing, Progress};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Serialize, Clone)]
pub struct Fail {
    pub code: String,
    pub message: String,
    /// True for a damaged .purr that carries repair data (the app offers "Try to repair").
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub repairable: bool,
}
impl Fail {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Fail { code: code.into(), message: message.into(), repairable: false }
    }
    /// Mark a "damaged" failure as repairable when the file is a .purr with repair data.
    fn for_archive(mut self, path: &Path) -> Self {
        if self.code == "damaged" && matches!(core::detect(path), Ok(core::Format::Purr)) && core::purr::repairable(path) {
            self.repairable = true;
        }
        self
    }
}
impl From<core::Error> for Fail {
    fn from(e: core::Error) -> Self {
        Fail::new(e.code(), e.to_string())
    }
}
impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Self {
        Fail::new("io", e.to_string())
    }
}

/// Run a job body; a crash inside becomes an ordinary failure instead of a job that never ends.
fn guarded(f: impl FnOnce() -> Result<Value, Fail>) -> Result<Value, Fail> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Err(Fail::new("other", "Something went wrong inside Purrzip. Nothing of yours was changed.")),
    }
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct JobState {
    pub kind: String,
    pub status: String,
    pub done: u64,
    pub total: u64,
    pub current: String,
    pub result: Option<Value>,
    pub error: Option<Fail>,
}

struct Job {
    cancel: AtomicBool,
    state: Mutex<JobState>,
}
impl Progress for Job {
    fn update(&self, done: u64, total: u64, current: &str) {
        let mut s = self.state.lock().unwrap();
        s.done = done;
        s.total = total;
        if !current.is_empty() {
            s.current = current.to_string();
        }
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub struct Service {
    jobs: Mutex<HashMap<u64, Arc<Job>>>,
    next: AtomicU64,
    cache: PathBuf,
    platform: String,
    /// Start places for the in-app folder browser (Android storage roots).
    roots: Vec<(String, PathBuf)>,
    downloads: Option<PathBuf>,
}

fn arg_str(a: &Value, k: &str) -> Result<String, Fail> {
    a.get(k).and_then(|v| v.as_str()).map(|s| s.to_string()).ok_or_else(|| Fail::new("bad_request", format!("missing {}", k)))
}
fn arg_opt(a: &Value, k: &str) -> Option<String> {
    a.get(k).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string())
}

/// "photos.tar.gz" -> "photos", "set.part1.rar" -> "set"
pub fn archive_stem(path: &Path) -> String {
    // A piece of a split set ("photos.7z.003") is named after the set.
    let base;
    let path = match core::split::piece_number(path) {
        Some((b, _)) => {
            base = b;
            base.as_path()
        }
        None => path,
    };
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "archive".into());
    let lower = name.to_lowercase();
    for ext in [".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tgz", ".tbz2", ".txz", ".zip", ".7z", ".purr", ".rar", ".tar", ".gz", ".bz2", ".xz", ".zst", ".iso", ".cab", ".jar", ".apk"] {
        if lower.ends_with(ext) && name.len() > ext.len() {
            let mut stem = name[..name.len() - ext.len()].to_string();
            let l = stem.to_lowercase();
            if let Some(i) = l.rfind(".part") {
                if i > 0 && l[i + 5..].chars().all(|c| c.is_ascii_digit()) {
                    stem.truncate(i);
                }
            }
            return stem;
        }
    }
    name
}

/// `dir/name` if free, else `dir/name (2)` etc. `ext` includes the dot or is empty.
pub fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{}{}", name, ext));
    if std::fs::symlink_metadata(&first).is_err() {
        return first;
    }
    for n in 2..10000 {
        let p = dir.join(format!("{} ({}){}", name, n, ext));
        if std::fs::symlink_metadata(&p).is_err() {
            return p;
        }
    }
    dir.join(format!("{}-{}{}", name, std::process::id(), ext))
}

fn kind_of(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "avif" | "ico" => "image",
        "mp4" | "webm" | "m4v" | "mov" | "ogv" => "video",
        "mp3" | "m4a" | "ogg" | "opus" | "wav" | "flac" | "aac" => "audio",
        "pdf" => "pdf",
        _ => "other",
    }
}

impl Service {
    pub fn new(cache: PathBuf, platform: &str, roots: Vec<(String, PathBuf)>, downloads: Option<PathBuf>) -> Self {
        let _ = std::fs::remove_dir_all(cache.join("previews"));
        let _ = std::fs::create_dir_all(&cache);
        Service { jobs: Mutex::new(HashMap::new()), next: AtomicU64::new(1), cache, platform: platform.into(), roots, downloads }
    }

    /// One entry point for every command.
    pub fn call(self: &Arc<Self>, cmd: &str, a: &Value) -> Result<Value, Fail> {
        match cmd {
            "info" => Ok(self.info()),
            "list" => self.list(a).map_err(|f| match a.get("path").and_then(|v| v.as_str()) {
                Some(p) => f.for_archive(Path::new(p)),
                None => f,
            }),
            "extract" => self.extract(a),
            "create" => self.create(a),
            "job" => self.job(a),
            "cancel" => {
                if let Some(j) = self.get_job(a)? {
                    j.cancel.store(true, Ordering::Relaxed);
                }
                Ok(json!({"ok": true}))
            }
            "forget" => {
                let id = a.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                self.jobs.lock().unwrap().remove(&id);
                Ok(json!({"ok": true}))
            }
            "preview" => self.preview(a).map_err(|f| match a.get("path").and_then(|v| v.as_str()) {
                Some(p) => f.for_archive(Path::new(p)),
                None => f,
            }),
            "repair" => self.repair(a),
            "test" => self.test(a),
            "browse" => self.browse(a),
            "places" => Ok(self.places()),
            "stat" => {
                let p = PathBuf::from(arg_str(a, "path")?);
                let m = std::fs::metadata(&p)?;
                Ok(json!({"path": p, "name": p.file_name().map(|s| s.to_string_lossy()), "is_dir": m.is_dir(), "size": m.len()}))
            }
            _ => Err(Fail::new("bad_request", format!("unknown command {}", cmd))),
        }
    }

    fn info(&self) -> Value {
        json!({
            "version": VERSION,
            "platform": self.platform,
            "downloads": self.downloads.clone().or_else(dirs::download_dir),
            "home": dirs::home_dir(),
            "in_app_browser": self.platform == "android",
        })
    }

    fn places(&self) -> Value {
        let mut v = Vec::new();
        for (label, p) in &self.roots {
            if p.is_dir() {
                v.push(json!({"label": label, "path": p}));
            }
        }
        if self.roots.is_empty() {
            for (label, p) in [
                ("Downloads", dirs::download_dir()),
                ("Documents", dirs::document_dir()),
                ("Desktop", dirs::desktop_dir()),
                ("Pictures", dirs::picture_dir()),
                ("Home", dirs::home_dir()),
            ] {
                if let Some(p) = p.filter(|p| p.is_dir()) {
                    v.push(json!({"label": label, "path": p}));
                }
            }
        }
        json!(v)
    }

    fn list(&self, a: &Value) -> Result<Value, Fail> {
        let path = PathBuf::from(arg_str(a, "path")?);
        let pw = arg_opt(a, "password");
        let l: Listing = core::list(&path, pw.as_deref())?;
        let size = match &l.split {
            Some(s) => s.bytes,
            None => std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        };
        let mut v = serde_json::to_value(&l).unwrap();
        // A split set is shown under its own name ("Videos.zip"), whichever piece was opened.
        let shown = core::split::piece_number(&path).filter(|_| l.split.is_some()).map(|(b, _)| b).unwrap_or_else(|| path.clone());
        v["name"] = json!(shown.file_name().map(|s| s.to_string_lossy()));
        v["stem"] = json!(archive_stem(&path));
        v["file_size"] = json!(size);
        v["label"] = json!(l.format.label());
        v["folder"] = json!(path.parent());
        Ok(v)
    }

    fn new_job(&self, kind: &str) -> (u64, Arc<Job>) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let job = Arc::new(Job {
            cancel: AtomicBool::new(false),
            state: Mutex::new(JobState { kind: kind.into(), status: "running".into(), ..Default::default() }),
        });
        self.jobs.lock().unwrap().insert(id, job.clone());
        (id, job)
    }

    fn finish(job: &Job, r: Result<Value, Fail>) {
        let mut s = job.state.lock().unwrap();
        match r {
            Ok(v) => {
                s.status = "done".into();
                s.result = Some(v);
            }
            Err(f) => {
                s.status = if f.code == "cancelled" { "cancelled".into() } else { "error".into() };
                s.error = Some(f);
            }
        }
    }

    fn get_job(&self, a: &Value) -> Result<Option<Arc<Job>>, Fail> {
        let id = a.get("id").and_then(|v| v.as_u64()).ok_or_else(|| Fail::new("bad_request", "missing id"))?;
        Ok(self.jobs.lock().unwrap().get(&id).cloned())
    }

    fn job(&self, a: &Value) -> Result<Value, Fail> {
        match self.get_job(a)? {
            Some(j) => Ok(serde_json::to_value(j.state.lock().unwrap().clone()).unwrap()),
            None => Err(Fail::new("no_job", "That job is gone.")),
        }
    }

    fn extract(self: &Arc<Self>, a: &Value) -> Result<Value, Fail> {
        let path = PathBuf::from(arg_str(a, "path")?);
        let dest_parent = PathBuf::from(arg_str(a, "dest")?);
        let pw = arg_opt(a, "password");
        let own_folder = a.get("own_folder").and_then(|v| v.as_bool()).unwrap_or(false);
        let only: Option<Vec<usize>> =
            a.get("only").and_then(|v| v.as_array()).map(|v| v.iter().filter_map(|x| x.as_u64().map(|n| n as usize)).collect());
        if !dest_parent.is_dir() {
            return Err(Fail::new("bad_dest", "That folder doesn't exist."));
        }
        let (id, job) = self.new_job("extract");
        std::thread::spawn(move || {
            let r = guarded(|| extract_job(&path, &dest_parent, pw.as_deref(), only.as_deref(), own_folder, &job)).map_err(|f| f.for_archive(&path));
            Service::finish(&job, r);
        });
        Ok(json!({"id": id}))
    }

    /// Check every file in an archive (unpacks to a temporary folder, compares checks, cleans up).
    fn test(self: &Arc<Self>, a: &Value) -> Result<Value, Fail> {
        let path = PathBuf::from(arg_str(a, "path")?);
        let pw = arg_opt(a, "password");
        let (id, job) = self.new_job("test");
        std::thread::spawn(move || {
            let r = guarded(|| core::test(&path, pw.as_deref(), &*job).map(|rep| json!({"path": path, "report": rep})).map_err(Fail::from))
                .map_err(|f| f.for_archive(&path));
            Service::finish(&job, r);
        });
        Ok(json!({"id": id}))
    }

    fn create(self: &Arc<Self>, a: &Value) -> Result<Value, Fail> {
        let sources: Vec<PathBuf> = a
            .get("sources")
            .and_then(|v| v.as_array())
            .map(|v| v.iter().filter_map(|x| x.as_str().map(PathBuf::from)).collect())
            .unwrap_or_default();
        if sources.is_empty() {
            return Err(Fail::new("bad_request", "Pick at least one file or folder."));
        }
        let mut opts: CreateOptions = serde_json::from_value(a.get("options").cloned().unwrap_or(Value::Null))
            .map_err(|e| Fail::new("bad_request", e.to_string()))?;
        if self.platform == "android" {
            opts.low_memory = true; // phones: smaller packs and dictionaries
        }
        let out_dir = PathBuf::from(arg_str(a, "out_dir")?);
        if !out_dir.is_dir() {
            return Err(Fail::new("bad_dest", "That folder doesn't exist."));
        }
        let mut name = arg_opt(a, "name").unwrap_or_else(|| "Archive".into());
        name = name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_").trim().to_string();
        let ext = match opts.format {
            core::CreateFormat::Zip => ".zip",
            core::CreateFormat::SevenZ => ".7z",
            core::CreateFormat::Purr => ".purr",
        };
        if name.to_lowercase().ends_with(ext) {
            name.truncate(name.len() - ext.len());
        }
        if name.is_empty() {
            name = "Archive".into();
        }
        let mut out = unique_path(&out_dir, &name, ext);
        if opts.split_size.is_some() {
            // Pieces are named out.001, out.002...: the name must be free for those too.
            let mut n = 2;
            while core::split::pieces_exist(&out) || std::fs::symlink_metadata(&out).is_ok() {
                out = out_dir.join(format!("{} ({}){}", name, n, ext));
                n += 1;
            }
        }
        let (id, job) = self.new_job("create");
        std::thread::spawn(move || {
            let r = guarded(|| {
                core::create(&sources, &out, &opts, &*job)
                    .map(|rep| json!({"path": rep.pieces.first().cloned().unwrap_or_else(|| out.clone()), "report": rep}))
                    .map_err(Fail::from)
            });
            if r.is_err() {
                let _ = std::fs::remove_file(format!("{}.part", out.display()));
            }
            Service::finish(&job, r);
        });
        Ok(json!({"id": id}))
    }

    /// Make a repaired copy of a damaged .purr next to it ("name (repaired).purr").
    fn repair(self: &Arc<Self>, a: &Value) -> Result<Value, Fail> {
        let path = PathBuf::from(arg_str(a, "path")?);
        let dir = path.parent().map(|p| p.to_path_buf()).ok_or_else(|| Fail::new("bad_request", "bad path"))?;
        let out = unique_path(&dir, &format!("{} (repaired)", archive_stem(&path)), ".purr");
        let (id, job) = self.new_job("repair");
        std::thread::spawn(move || {
            let r = guarded(|| match core::purr::repair(&path, &out, &*job) {
                Ok(rep) if rep.clean => Ok(json!({"path": path, "report": rep})),
                Ok(rep) => Ok(json!({"path": out, "report": rep})),
                Err(e) => Err(Fail::from(e)),
            });
            if r.is_err() {
                let _ = std::fs::remove_file(&out);
            }
            Service::finish(&job, r);
        });
        Ok(json!({"id": id}))
    }

    fn preview(&self, a: &Value) -> Result<Value, Fail> {
        let path = PathBuf::from(arg_str(a, "path")?);
        let pw = arg_opt(a, "password");
        let index = a.get("index").and_then(|v| v.as_u64()).ok_or_else(|| Fail::new("bad_request", "missing index"))? as usize;
        let listing = core::list(&path, pw.as_deref())?;
        let e = listing.entries.get(index).ok_or_else(|| Fail::new("bad_request", "no such file"))?;
        if e.is_dir {
            return Err(Fail::new("bad_request", "That's a folder."));
        }
        if e.size > 300 << 20 {
            return Err(Fail::new("too_big", "That file is too big to preview. Extract it instead."));
        }
        let base = self.cache.join("previews");
        std::fs::create_dir_all(&base)?;
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("p{}", n));
        std::fs::create_dir_all(&dir)?;
        let quiet = core::Quiet::default();
        let report = core::extract_listed(&path, &listing, &dir, pw.as_deref(), Some(&[index]), &quiet)?;
        let rel = report
            .written
            .iter()
            .find(|(i, _)| *i == index)
            .map(|(_, r)| r.clone())
            .ok_or_else(|| Fail::new("skipped", "That file can't be previewed."))?;
        let file = dir.join(&rel);
        let kind = kind_of(&e.path);
        let mut out = json!({"file": file, "kind": kind, "name": e.path.rsplit('/').next(), "size": e.size});
        if kind == "other" {
            let bytes = std::fs::read(&file)?;
            let cut = &bytes[..bytes.len().min(200_000)];
            let looks_text = !cut.iter().take(8192).any(|&b| b == 0) && std::str::from_utf8(&cut[..cut.len().min(8000)]).is_ok_or_cut();
            if looks_text {
                out["kind"] = json!("text");
                out["text"] = json!(String::from_utf8_lossy(cut));
                out["cut"] = json!(bytes.len() > 200_000);
            }
        }
        Ok(out)
    }

    fn browse(&self, a: &Value) -> Result<Value, Fail> {
        let dir = match arg_opt(a, "dir") {
            Some(d) => PathBuf::from(d),
            None => self.roots.first().map(|r| r.1.clone()).or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("/")),
        };
        let show_hidden = a.get("hidden").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut items = Vec::new();
        for e in std::fs::read_dir(&dir)?.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            let Ok(m) = e.metadata() else { continue };
            let modified = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
            items.push(json!({"name": name, "path": e.path(), "is_dir": m.is_dir(), "size": m.len(), "modified": modified}));
        }
        items.sort_by(|x, y| {
            let dx = x["is_dir"].as_bool().unwrap_or(false);
            let dy = y["is_dir"].as_bool().unwrap_or(false);
            dy.cmp(&dx).then_with(|| x["name"].as_str().unwrap_or("").to_lowercase().cmp(&y["name"].as_str().unwrap_or("").to_lowercase()))
        });
        // Don't offer to climb above a storage root on phones.
        let parent = if self.roots.iter().any(|r| r.1 == dir) { None } else { dir.parent().map(|p| p.to_path_buf()) };
        Ok(json!({"dir": dir, "parent": parent, "items": items}))
    }
}

/// Text check that tolerates a multi-byte character cut at the end.
trait Utf8Cut {
    fn is_ok_or_cut(&self) -> bool;
}
impl Utf8Cut for Result<&str, std::str::Utf8Error> {
    fn is_ok_or_cut(&self) -> bool {
        match self {
            Ok(_) => true,
            Err(e) => e.error_len().is_none(),
        }
    }
}

/// `own_folder`: always unpack into a new folder named after the archive (right-click \"Extract to new folder\").
fn extract_job(path: &Path, dest_parent: &Path, pw: Option<&str>, only: Option<&[usize]>, own_folder: bool, job: &Job) -> Result<Value, Fail> {
    let listing = core::list(path, pw)?;
    let wanted: Vec<&core::Entry> = match only {
        None => listing.entries.iter().collect(),
        Some(ix) => listing.entries.iter().filter(|e| ix.contains(&e.index)).collect(),
    };
    let stem = archive_stem(path);
    // Tidy result: if everything sits under one top item, don't wrap it in another folder.
    let tops: std::collections::BTreeSet<String> = wanted
        .iter()
        .filter(|e| !e.special)
        .map(|e| e.path.split('/').find(|p| !p.is_empty() && *p != ".").unwrap_or("").to_string())
        .collect();
    if tops.len() == 1 && only.is_none() && !own_folder {
        let tmp = unique_path(dest_parent, &format!(".purrzip-{}", stem), "");
        std::fs::create_dir(&tmp)?;
        let report = match core::extract_listed(path, &listing, &tmp, pw, None, job) {
            Ok(r) => r,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(e.into());
            }
        };
        let inside: Vec<_> = std::fs::read_dir(&tmp)?.filter_map(|e| e.ok()).collect();
        let final_path = if inside.len() == 1 {
            let item = inside[0].path();
            let name = item.file_name().unwrap().to_string_lossy().into_owned();
            let (base, ext) = if item.is_dir() {
                (name.clone(), String::new())
            } else {
                match name.rfind('.') {
                    Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
                    _ => (name.clone(), String::new()),
                }
            };
            let target = unique_path(dest_parent, &base, &ext);
            std::fs::rename(&item, &target)?;
            let _ = std::fs::remove_dir(&tmp);
            target
        } else {
            let target = unique_path(dest_parent, &stem, "");
            std::fs::rename(&tmp, &target)?;
            target
        };
        return Ok(json!({"folder": final_path, "report": report, "single": inside.len() == 1}));
    }
    let target = unique_path(dest_parent, &stem, "");
    std::fs::create_dir(&target)?;
    match core::extract_listed(path, &listing, &target, pw, only, job) {
        Ok(report) => Ok(json!({"folder": target, "report": report, "single": false})),
        Err(e) => {
            // Leave nothing half-done behind when stopped.
            if matches!(e, core::Error::Cancelled) {
                let _ = std::fs::remove_dir_all(&target);
            } else if std::fs::read_dir(&target).map(|mut d| d.next().is_none()).unwrap_or(false) {
                let _ = std::fs::remove_dir(&target);
            }
            Err(e.into())
        }
    }
}
