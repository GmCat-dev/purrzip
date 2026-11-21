//! Command-line Purrzip (used by the self-tests; handy on servers too).
//! purrzip-cli list <archive> [password]
//! purrzip-cli extract <archive> <dest> [password] [--only 0,3,5]
//! purrzip-cli test <archive> [password]
//! purrzip-cli create <zip|7z> <store|fast|normal|max> <out> [--password X] [--hide-names] <sources...>

use purrzip_core::*;
use std::path::{Path, PathBuf};

struct Print;
impl Progress for Print {}

fn fail(e: Error) -> ! {
    println!("{}", serde_json::json!({"ok": false, "code": e.code(), "message": e.to_string()}));
    std::process::exit(2);
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let cmd = a.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "list" => {
            let pw = a.get(2).map(|s| s.as_str());
            match list(Path::new(&a[1]), pw) {
                Ok(l) => println!("{}", serde_json::to_string(&l).unwrap()),
                Err(e) => fail(e),
            }
        }
        "extract" => {
            let mut pw = None;
            let mut only: Option<Vec<usize>> = None;
            let mut i = 3;
            while i < a.len() {
                if a[i] == "--only" {
                    only = Some(a[i + 1].split(',').filter_map(|x| x.parse().ok()).collect());
                    i += 2;
                } else {
                    pw = Some(a[i].clone());
                    i += 1;
                }
            }
            match extract(Path::new(&a[1]), Path::new(&a[2]), pw.as_deref(), only.as_deref(), &Print) {
                Ok(r) => println!("{}", serde_json::to_string(&serde_json::json!({"ok": true, "report": r})).unwrap()),
                Err(e) => fail(e),
            }
        }
        "test" => match test(Path::new(&a[1]), a.get(2).map(|s| s.as_str()), &Print) {
            Ok(r) => println!("{}", serde_json::json!({"ok": true, "files": r.files, "bytes": r.bytes})),
            Err(e) => fail(e),
        },
        "create" => {
            let format = if a[1] == "7z" { CreateFormat::SevenZ } else { CreateFormat::Zip };
            let level = match a[2].as_str() {
                "store" => Level::Store,
                "fast" => Level::Fast,
                "max" => Level::Max,
                _ => Level::Normal,
            };
            let out = PathBuf::from(&a[3]);
            let mut password = None;
            let mut hide_names = false;
            let mut sources = Vec::new();
            let mut i = 4;
            while i < a.len() {
                match a[i].as_str() {
                    "--password" => {
                        password = Some(a[i + 1].clone());
                        i += 1;
                    }
                    "--hide-names" => hide_names = true,
                    s => sources.push(PathBuf::from(s)),
                }
                i += 1;
            }
            let opts = CreateOptions { format, level, password, hide_names };
            match create(&sources, &out, &opts, &Print) {
                Ok(r) => println!("{}", serde_json::to_string(&serde_json::json!({"ok": true, "report": r})).unwrap()),
                Err(e) => fail(e),
            }
        }
        _ => {
            eprintln!("usage: purrzip-cli list|extract|test|create ...");
            std::process::exit(1);
        }
    }
}
