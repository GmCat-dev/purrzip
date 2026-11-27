//! Command-line Purrzip (used by the self-tests; handy on servers too).
//! purrzip-cli list <archive> [password]
//! purrzip-cli extract <archive> <dest> [password] [--only 0,3,5]
//! purrzip-cli test <archive> [password]
//! purrzip-cli create <zip|7z|purr> <store|fast|normal|max> <out> [--password X] [--hide-names] [--repair] [--low-memory] <sources...>
//! purrzip-cli repair <damaged.purr> <repaired.purr>      (uses the archive's repair data; never changes the original)
//! purrzip-cli verify <archive.purr>                      (checks an archive against its repair data)

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
            let format = match a[1].as_str() {
                "7z" => CreateFormat::SevenZ,
                "purr" => CreateFormat::Purr,
                _ => CreateFormat::Zip,
            };
            let level = match a[2].as_str() {
                "store" => Level::Store,
                "fast" => Level::Fast,
                "max" => Level::Max,
                _ => Level::Normal,
            };
            let out = PathBuf::from(&a[3]);
            let mut password = None;
            let mut hide_names = false;
            let (mut repair, mut low_memory) = (false, false);
            let mut split_size = None;
            let mut sources = Vec::new();
            let mut i = 4;
            while i < a.len() {
                match a[i].as_str() {
                    "--password" => {
                        password = Some(a[i + 1].clone());
                        i += 1;
                    }
                    "--hide-names" => hide_names = true,
                    "--repair" => repair = true,
                    "--low-memory" => low_memory = true,
                    "--split" => {
                        split_size = Some(purrzip_core::split::parse_size(&a[i + 1]).unwrap_or_else(|| {
                            eprintln!("--split needs a size like 450MB or 2GB");
                            std::process::exit(1)
                        }));
                        i += 1;
                    }
                    s => sources.push(PathBuf::from(s)),
                }
                i += 1;
            }
            let opts = CreateOptions { format, level, password, hide_names, repair, low_memory, split_size };
            match create(&sources, &out, &opts, &Print) {
                Ok(r) => println!("{}", serde_json::to_string(&serde_json::json!({"ok": true, "report": r})).unwrap()),
                Err(e) => fail(e),
            }
        }
        "repair" => match purr::repair(Path::new(&a[1]), Path::new(&a[2]), &Print) {
            Ok(r) => println!("{}", serde_json::json!({"ok": true, "report": r})),
            Err(e) => fail(e),
        },
        "verify" => match purr::verify(Path::new(&a[1]), &Print) {
            Ok(r) => println!("{}", serde_json::json!({"ok": true, "report": r})),
            Err(e) => fail(e),
        },
        _ => {
            eprintln!("usage: purrzip-cli list|extract|test|create|repair|verify ...");
            std::process::exit(1);
        }
    }
}
