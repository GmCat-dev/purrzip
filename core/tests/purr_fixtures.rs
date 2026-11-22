//! Cross-platform check for the .purr format.
//!
//! The archives in tests/fixtures were made on Linux (x86-64). This test runs on every platform
//! Purrzip is built for (Windows, macOS on Apple chips, Linux) and must unpack them to exactly
//! the files in tests/fixtures/sample-src. That covers the parts where two machines could in
//! principle disagree: the photo restorer, the squeezing methods, the lock and the repair maths.

use purrzip_core::{create, extract, list, purr, CreateFormat, CreateOptions, Level, Quiet};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("tests").join("fixtures")
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                out.insert(format!("{}/", rel), Vec::new());
                walk(&p, root, out);
            } else {
                out.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        Tmp(purrzip_core::temp_folder("fixtures").unwrap())
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn archives_made_on_linux_unpack_identically_here() {
    let want = snapshot(&fixtures().join("sample-src"));
    assert!(want.len() >= 9, "fixture source is incomplete");
    for (name, password) in [("sample.purr", None), ("sample-fast.purr", None), ("sample-max.purr", None), ("sample-locked.purr", Some("purr-test")), ("sample-repair.purr", None)] {
        let t = Tmp::new();
        let archive = fixtures().join(name);
        let l = list(&archive, password).unwrap_or_else(|e| panic!("{}: {}", name, e));
        assert_eq!(l.format, purrzip_core::Format::Purr, "{}", name);
        extract(&archive, &t.0, password, None, &Quiet::default()).unwrap_or_else(|e| panic!("{}: {}", name, e));
        assert_eq!(snapshot(&t.0.join("sample-src")), want, "{} did not unpack to the original files", name);
    }
    // the locked one really is locked
    assert!(list(&fixtures().join("sample-locked.purr"), None).is_err());
    assert!(list(&fixtures().join("sample-locked.purr"), Some("wrong")).is_err());
}

#[test]
fn a_damaged_fixture_is_repaired_here() {
    let t = Tmp::new();
    let good = fs::read(fixtures().join("sample-repair.purr")).unwrap();
    let mut bad = good.clone();
    bad[7] ^= 0x55;
    for b in &mut bad[20_000..21_500] {
        *b = 0;
    }
    let broken = t.0.join("broken.purr");
    fs::write(&broken, &bad).unwrap();
    assert!(extract(&broken, &t.0, None, None, &Quiet::default()).is_err());
    let fixed = t.0.join("fixed.purr");
    let report = purr::repair(&broken, &fixed, &Quiet::default()).unwrap();
    assert!(report.repaired);
    assert_eq!(fs::read(&fixed).unwrap(), good, "the repaired copy must equal the archive as it was made on Linux");
}

#[test]
fn archives_made_here_unpack_here() {
    let src = fixtures().join("sample-src");
    let want = snapshot(&src);
    for level in [Level::Fast, Level::Normal, Level::Max] {
        for password in [None, Some("pw ☕")] {
            let t = Tmp::new();
            let out = t.0.join("a.purr");
            let opts = CreateOptions { format: CreateFormat::Purr, level, password: password.map(String::from), hide_names: false, repair: true, low_memory: level == Level::Fast };
            create(&[src.clone()], &out, &opts, &Quiet::default()).unwrap();
            let dest = t.0.join("out");
            fs::create_dir_all(&dest).unwrap();
            extract(&out, &dest, password, None, &Quiet::default()).unwrap();
            assert_eq!(snapshot(&dest.join("sample-src")), want, "{:?} {:?}", level, password);
        }
    }
}
