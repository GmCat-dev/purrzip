//! Local test server: serves the app screen (app/ui) and answers its commands with the
//! real Purrzip service, so browser tests exercise real code. Listens on 127.0.0.1 only.
//! PZ_PORT (default 4231), PZ_UI (ui folder), PZ_PLATFORM (default "android": in-app file browser),
//! PZ_ROOT (start folder for the browser, acts like phone storage).
use purrzip_service::Service;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tiny_http::{Header, Response, Server};

fn ctype(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn main() {
    let port = std::env::var("PZ_PORT").unwrap_or_else(|_| "4231".into());
    let ui = PathBuf::from(std::env::var("PZ_UI").unwrap_or_else(|_| "app/ui".into())).canonicalize().expect("ui folder");
    let platform = std::env::var("PZ_PLATFORM").unwrap_or_else(|_| "android".into());
    let cache = std::env::temp_dir().join(format!("purrzip-dev-cache-{}", port));
    let roots: Vec<(String, PathBuf)> = match std::env::var("PZ_ROOT") {
        Ok(r) => vec![("Phone storage".into(), PathBuf::from(r))],
        Err(_) => vec![],
    };
    let downloads = roots.first().map(|r| r.1.join("Download")).filter(|p| p.is_dir());
    let svc = Arc::new(Service::new(cache.clone(), &platform, roots, downloads));
    let server = Server::http(format!("127.0.0.1:{}", port)).expect("port");
    eprintln!("purrzip dev server on http://127.0.0.1:{}/", port);
    for mut req in server.incoming_requests() {
        let url = req.url().to_string();
        let (path, query) = url.split_once('?').unwrap_or((&url, ""));
        if let Some(cmd) = path.strip_prefix("/api/") {
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);
            let args: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            let out = match svc.call(cmd, &args) {
                Ok(v) => serde_json::json!({"ok": true, "data": v}),
                Err(f) => serde_json::json!({"ok": false, "error": f}),
            };
            let _ = req.respond(Response::from_string(out.to_string()).with_header(Header::from_bytes("Content-Type", "application/json").unwrap()));
            continue;
        }
        if path == "/file" {
            // Preview files only, from the cache folder.
            let p = query.strip_prefix("path=").map(|s| percent_decode(s)).unwrap_or_default();
            let p = PathBuf::from(p);
            let ok = p.canonicalize().map(|c| c.starts_with(cache.canonicalize().unwrap())).unwrap_or(false);
            if ok {
                if let Ok(f) = std::fs::File::open(&p) {
                    let _ = req.respond(Response::from_file(f).with_header(Header::from_bytes("Content-Type", ctype(&p)).unwrap()));
                    continue;
                }
            }
            let _ = req.respond(Response::from_string("no").with_status_code(404));
            continue;
        }
        let rel = if path == "/" { "index.html" } else { path.trim_start_matches('/') };
        let file = ui.join(percent_decode(rel));
        match file.canonicalize() {
            Ok(c) if c.starts_with(&ui) && c.is_file() => {
                let f = std::fs::File::open(&c).unwrap();
                let _ = req.respond(Response::from_file(f).with_header(Header::from_bytes("Content-Type", ctype(&c)).unwrap()));
            }
            _ => {
                let _ = req.respond(Response::from_string("not found").with_status_code(404));
            }
        }
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
