//! Purrzip app shell: one command (`pz`) forwards to the shared Purrzip service.
//! Copyright (C) 2026 GmCat. AGPL-3.0-or-later (see LICENSE).

use purrzip_service::{Fail, Service};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

struct AppState {
    svc: Arc<Service>,
    /// Files the system asked us to open at launch (Open with…, double-click).
    startup: Mutex<Vec<String>>,
}

fn paths_from_args(args: &[String]) -> Vec<String> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .filter(|a| PathBuf::from(a).exists())
        .cloned()
        .collect()
}

#[tauri::command]
async fn pz(state: tauri::State<'_, AppState>, cmd: String, args: Value) -> Result<Value, Fail> {
    if cmd == "startup" {
        let paths: Vec<String> = std::mem::take(&mut *state.startup.lock().unwrap());
        return Ok(json!({ "paths": paths }));
    }
    let svc = state.svc.clone();
    tauri::async_runtime::spawn_blocking(move || svc.call(&cmd, &args))
        .await
        .map_err(|e| Fail { code: "other".into(), message: e.to_string() })?
}

#[cfg(target_os = "android")]
fn platform_roots() -> (Vec<(String, PathBuf)>, Option<PathBuf>) {
    let main = PathBuf::from("/storage/emulated/0");
    let mut roots = vec![("Phone storage".to_string(), main.clone())];
    // Memory cards show up as /storage/XXXX-XXXX.
    if let Ok(rd) = std::fs::read_dir("/storage") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.len() == 9 && name.as_bytes()[4] == b'-' {
                roots.push(("Memory card".to_string(), e.path()));
            }
        }
    }
    (roots, Some(main.join("Download")))
}
#[cfg(not(target_os = "android"))]
fn platform_roots() -> (Vec<(String, PathBuf)>, Option<PathBuf>) {
    (vec![], None)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        // A second launch (e.g. double-clicking another archive) hands its files to the open window.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let paths = paths_from_args(&argv);
            if !paths.is_empty() {
                let _ = app.emit("pz-open", paths);
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }));
    }
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let cache = app.path().app_cache_dir().unwrap_or_else(|_| std::env::temp_dir().join("purrzip"));
            let (roots, downloads) = platform_roots();
            let platform = std::env::consts::OS;
            let svc = Arc::new(Service::new(cache, platform, roots, downloads));
            let args: Vec<String> = std::env::args().collect();
            app.manage(AppState { svc, startup: Mutex::new(paths_from_args(&args)) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![pz])
        .build(tauri::generate_context!())
        .expect("error while starting Purrzip");
    app.run(|_app, _event| {
        // macOS hands files over with an Opened event instead of arguments.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Opened { urls } = _event {
            let paths: Vec<String> = urls.iter().filter_map(|u| u.to_file_path().ok()).map(|p| p.to_string_lossy().into_owned()).collect();
            if let Some(st) = _app.try_state::<AppState>() {
                st.startup.lock().unwrap().extend(paths.clone());
            }
            let _ = _app.emit("pz-open", paths);
        }
    });
}
