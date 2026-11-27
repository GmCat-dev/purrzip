//! The .purr archive format (Purrzip's own). See docs/PURR-FORMAT.md.
//!
//! What it adds over .zip and .7z: data that repeats anywhere in the archive is stored once,
//! JPEG photos are shrunk without changing a single pixel, each kind of file gets the squeezing
//! method that suits it, a lock hides names as well as contents, and optional repair data lets
//! a damaged archive fix itself.

mod chunk;
pub mod codec;
mod jpeg;
mod pool;
mod read;
pub mod recovery;
mod seal;
pub mod wire;
mod write;

use crate::safe::Sink;
use crate::{Entry, Error, Level, Progress, Result, Wanted};
use std::path::Path;

pub use recovery::RepairReport;
pub use wire::Stats;
pub(crate) use write::write_purr;

#[derive(Debug, Clone)]
pub struct PurrOptions {
    pub level: Level,
    pub password: Option<String>,
    /// Add repair data (about 3% bigger) so a damaged archive can fix itself.
    pub repair: bool,
    /// Phones: smaller packs and dictionaries, fewer workers.
    pub low_memory: bool,
}

/// Extra facts about a .purr, for the app to show.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PurrInfo {
    pub repair_data: bool,
    pub locked: bool,
    pub stats: Option<Stats>,
    pub made_by: String,
}

pub fn is_purr(head: &[u8]) -> bool {
    head.len() >= 8 && head[..8] == wire::MAGIC
}

/// True when a damaged file still carries usable repair data.
pub fn repairable(path: &Path) -> bool {
    recovery::has_recovery(path)
}

fn hint(e: Error, path: &Path) -> Error {
    match e {
        Error::Damaged(m) if repairable(path) => Error::Damaged(format!("{}. It has repair data, so Purrzip can try to fix it", m)),
        other => other,
    }
}

pub fn list(path: &Path, password: Option<&str>) -> Result<(Vec<Entry>, PurrInfo)> {
    let a = read::open(path, password).map_err(|e| hint(e, path))?;
    let info = PurrInfo { repair_data: a.has_repair_data, locked: a.encrypted(), stats: a.index.stats.clone(), made_by: a.index.creator.clone() };
    Ok((a.entries(), info))
}

pub fn extract(path: &Path, password: Option<&str>, wanted: &Wanted, sink: &mut Sink) -> Result<()> {
    let a = read::open(path, password).map_err(|e| hint(e, path))?;
    a.extract(wanted, sink).map_err(|e| hint(e, path))
}

/// Write a repaired copy of `src` to `dst` using its repair data. `src` is not changed.
pub fn repair(src: &Path, dst: &Path, progress: &dyn Progress) -> Result<RepairReport> {
    recovery::repair(src, dst, progress)
}

/// Check an archive against its repair data.
pub fn verify(path: &Path, progress: &dyn Progress) -> Result<RepairReport> {
    recovery::verify(path, progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::{create, gather, CreateFormat, CreateOptions, Item};
    use crate::{extract, list, Quiet};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    const PHOTO: &[u8] = include_bytes!("../../../tests/fixtures/photo.jpg");
    const PHOTO_PROG: &[u8] = include_bytes!("../../../tests/fixtures/photo-progressive.jpg");

    fn noise(n: usize, seed: u64) -> Vec<u8> {
        let mut x = seed | 1;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect()
    }

    struct Tmp(PathBuf);
    impl Tmp {
        fn new() -> Tmp {
            Tmp(crate::temp_folder("purrtest").unwrap())
        }
        fn p(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A folder with a bit of everything. `big` adds larger files (several packs at small pack sizes).
    fn tree(t: &Tmp, big: bool) -> PathBuf {
        let root = t.p("stuff");
        fs::create_dir_all(root.join("docs/deep")).unwrap();
        fs::create_dir_all(root.join("pics")).unwrap();
        fs::create_dir_all(root.join("empty dir")).unwrap();
        let text: String = (0..if big { 60000 } else { 3000 }).map(|i| format!("line {} says meow {} times\n", i, i % 13)).collect();
        fs::write(root.join("docs/notes.txt"), &text).unwrap();
        fs::write(root.join("docs/deep/notes copy.txt"), &text).unwrap();
        fs::write(root.join("docs/other.md"), "# hi\nshort file\n").unwrap();
        fs::write(root.join("docs/ünï ☕.txt"), "unicode name").unwrap();
        fs::write(root.join("empty.txt"), "").unwrap();
        fs::write(root.join("pics/photo.jpg"), PHOTO).unwrap();
        fs::write(root.join("pics/photo again.jpg"), PHOTO).unwrap();
        fs::write(root.join("pics/progressive.jpg"), PHOTO_PROG).unwrap();
        fs::write(root.join("pics/not really.jpg"), b"\xFF\xD8\xFFthis is not a picture at all, just bytes pretending").unwrap();
        let blob = noise(if big { 700_000 } else { 40_000 }, 5);
        fs::write(root.join("random.bin"), &blob).unwrap();
        fs::write(root.join("docs/random twin.bin"), &blob).unwrap();
        let mut prog = b"\x7fELF\x02\x01\x01".to_vec();
        prog.resize(18, 0);
        prog.extend_from_slice(&[62, 0]);
        prog.extend(text.bytes().take(20000).map(|b| b.wrapping_mul(7)));
        fs::write(root.join("program"), &prog).unwrap();
        root
    }

    fn snapshot(root: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
        let mut m = BTreeMap::new();
        for e in walkdir::WalkDir::new(root) {
            let e = e.unwrap();
            let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if e.file_type().is_dir() {
                m.insert(format!("{}/", rel), Vec::new());
            } else {
                m.insert(rel, fs::read(e.path()).unwrap());
            }
        }
        m
    }

    fn make(t: &Tmp, root: &std::path::Path, name: &str, level: Level, password: Option<&str>, repair: bool) -> PathBuf {
        let out = t.p(name);
        let opts = CreateOptions { format: CreateFormat::Purr, level, password: password.map(String::from), hide_names: false, repair, low_memory: false, split_size: None };
        let rep = create(&[root.to_path_buf()], &out, &opts, &Quiet::default()).unwrap();
        assert!(rep.purr.is_some());
        out
    }

    fn unpack(t: &Tmp, archive: &std::path::Path, tag: &str, password: Option<&str>) -> crate::Result<PathBuf> {
        let dest = t.p(tag);
        fs::create_dir_all(&dest).unwrap();
        extract(archive, &dest, password, None, &Quiet::default()).map(|_| dest)
    }

    #[test]
    fn every_level_round_trips() {
        let t = Tmp::new();
        let root = tree(&t, true);
        let want = snapshot(&root);
        for (i, level) in [Level::Store, Level::Fast, Level::Normal, Level::Max].into_iter().enumerate() {
            let a = make(&t, &root, &format!("a{}.purr", i), level, None, false);
            let out = unpack(&t, &a, &format!("out{}", i), None).unwrap();
            assert_eq!(snapshot(&out.join("stuff")), want, "level {:?}", level);
            let l = list(&a, None).unwrap();
            assert_eq!(l.format, crate::Format::Purr);
            assert_eq!(l.entries.iter().filter(|e| !e.is_dir).count(), 12);
        }
    }

    #[test]
    fn repeats_are_stored_once_and_photos_shrink() {
        let t = Tmp::new();
        let root = tree(&t, true);
        let a = make(&t, &root, "a.purr", Level::Normal, None, false);
        let info = list(&a, None).unwrap().purr.unwrap();
        let s = info.stats.unwrap();
        // twins: notes copy (text), random twin (700 KB), photo again
        assert!(s.dup_bytes >= 700_000 + PHOTO.len() as u64, "dup bytes {}", s.dup_bytes);
        assert_eq!(s.jpeg_count, 2, "two different real photos");
        assert!(s.jpeg_out < s.jpeg_in * 90 / 100, "photos {} -> {}", s.jpeg_in, s.jpeg_out);
        // the whole archive: one copy of the noise, shrunk photos, squeezed text
        let size = fs::metadata(&a).unwrap().len();
        assert!(size < 700_000 + 90_000 + 60_000, "archive is {} bytes", size);
    }

    #[test]
    fn lock_hides_names_and_contents() {
        let t = Tmp::new();
        let root = tree(&t, false);
        let a = make(&t, &root, "locked.purr", Level::Normal, Some("Paw$ 12 ☕"), false);
        assert!(matches!(list(&a, None), Err(Error::NeedPassword)));
        assert!(matches!(list(&a, Some("nope")), Err(Error::WrongPassword)));
        assert!(matches!(unpack(&t, &a, "x1", Some("paw$ 12 ☕")), Err(Error::WrongPassword)));
        let bytes = fs::read(&a).unwrap();
        for needle in [&b"notes"[..], b"photo", b"random", b"stuff", b"meow"] {
            assert!(!bytes.windows(needle.len()).any(|w| w == needle), "{:?} is readable in the locked file", String::from_utf8_lossy(needle));
        }
        let out = unpack(&t, &a, "x2", Some("Paw$ 12 ☕")).unwrap();
        assert_eq!(snapshot(&out.join("stuff")), snapshot(&root));
        assert!(list(&a, Some("Paw$ 12 ☕")).unwrap().purr.unwrap().locked);
        // Changing one byte of locked data is noticed, and it is not blamed on the password.
        let mut bad = bytes.clone();
        let mid = bad.len() / 2;
        bad[mid] ^= 1;
        fs::write(t.p("bad.purr"), &bad).unwrap();
        let r = unpack(&t, &t.p("bad.purr"), "x3", Some("Paw$ 12 ☕"));
        assert!(matches!(r, Err(Error::Damaged(_))), "{:?}", r.map(|_| ()));
    }

    #[test]
    fn hostile_names_stay_inside_the_folder() {
        let t = Tmp::new();
        let real = t.p("real.txt");
        fs::write(&real, "payload").unwrap();
        let item = |name: &str| Item { src: real.clone(), name: name.to_string(), is_dir: false, size: 7 };
        let items = vec![item("../escape.txt"), item("/abs.txt"), item("ok/../../escape2.txt"), item("C:\\win.txt"), item("con.txt"), item("fine/ok.txt"), item("fine/OK.txt")];
        let a = t.p("evil.purr");
        let opts = PurrOptions { level: Level::Fast, password: None, repair: false, low_memory: false };
        write_purr(&items, &a, &opts, &Quiet::default(), &mut Vec::new()).unwrap();
        let dest = t.p("out/inner");
        fs::create_dir_all(&dest).unwrap();
        let rep = extract(&a, &dest, None, None, &Quiet::default()).unwrap();
        assert_eq!(rep.skipped.len(), 2, "{:?}", rep.skipped);
        let got: Vec<String> = snapshot(&dest).into_keys().collect();
        assert!(got.contains(&"abs.txt".to_string()) && got.contains(&"C_/win.txt".to_string()) && got.contains(&"_con.txt".to_string()), "{:?}", got);
        assert!(got.contains(&"fine/ok.txt".to_string()) && got.contains(&"fine/OK (2).txt".to_string()), "{:?}", got);
        assert!(!t.p("out/escape.txt").exists() && !t.p("escape.txt").exists() && !t.p("out/escape2.txt").exists());
        assert_eq!(fs::read_dir(t.p("out")).unwrap().count(), 1);
    }

    #[test]
    fn only_the_asked_files_come_out() {
        let t = Tmp::new();
        let root = tree(&t, false);
        let a = make(&t, &root, "a.purr", Level::Normal, None, false);
        let l = list(&a, None).unwrap();
        let pick: Vec<usize> = l.entries.iter().filter(|e| e.path.ends_with("photo again.jpg") || e.path.ends_with("other.md") || e.path.ends_with("empty.txt")).map(|e| e.index).collect();
        assert_eq!(pick.len(), 3);
        let dest = t.p("some");
        fs::create_dir_all(&dest).unwrap();
        extract(&a, &dest, None, Some(&pick), &Quiet::default()).unwrap();
        let got = snapshot(&dest);
        let files: Vec<&String> = got.keys().filter(|k| !k.ends_with('/')).collect();
        assert_eq!(files, vec!["stuff/docs/other.md", "stuff/empty.txt", "stuff/pics/photo again.jpg"]);
        assert_eq!(got["stuff/pics/photo again.jpg"], PHOTO);
        // picking a folder brings everything under it
        let pics: Vec<usize> = l.entries.iter().filter(|e| e.path == "stuff/pics").map(|e| e.index).collect();
        let dest2 = t.p("pics-only");
        fs::create_dir_all(&dest2).unwrap();
        extract(&a, &dest2, None, Some(&pics), &Quiet::default()).unwrap();
        assert_eq!(snapshot(&dest2).keys().filter(|k| !k.ends_with('/')).count(), 4);
    }

    #[test]
    fn damage_is_never_silent() {
        // Flip one bit at a time all over a small archive. Every time, unpacking must either
        // fail cleanly or give back exactly the original files. Never a crash, never wrong data.
        let t = Tmp::new();
        let root = tree(&t, false);
        let want = snapshot(&root);
        let a = make(&t, &root, "a.purr", Level::Normal, None, false);
        let bytes = fs::read(&a).unwrap();
        let n = bytes.len();
        let mut positions: Vec<usize> = (0..160.min(n)).collect(); // header and the first pack bytes
        positions.extend((n.saturating_sub(700)..n).step_by(1)); // table of contents + footer
        positions.extend((160..n.saturating_sub(700)).step_by(97)); // through the packs
        let (mut failed, mut survived) = (0, 0);
        for (round, pos) in positions.into_iter().enumerate() {
            let mut m = bytes.clone();
            m[pos] ^= 1 << (round % 8);
            let f = t.p("m.purr");
            fs::write(&f, &m).unwrap();
            let dest = t.p(&format!("m{}", round));
            fs::create_dir_all(&dest).unwrap();
            match extract(&f, &dest, None, None, &Quiet::default()) {
                Err(_) => failed += 1,
                Ok(_) => {
                    assert_eq!(snapshot(&dest.join("stuff")), want, "a flipped bit at byte {} changed the result without an error", pos);
                    survived += 1;
                }
            }
            let _ = fs::remove_dir_all(&dest);
        }
        assert!(failed > 100, "only {} flips were noticed ({} harmless)", failed, survived);
        // cutting the file short anywhere is an error too
        for cut in [0usize, 10, 95, 96, 200, n / 2, n - 65, n - 64, n - 1] {
            let f = t.p("cut.purr");
            fs::write(&f, &bytes[..cut]).unwrap();
            assert!(list(&f, None).is_err(), "cut at {}", cut);
        }
    }

    #[test]
    fn repair_data_end_to_end() {
        let t = Tmp::new();
        let root = tree(&t, true);
        let want = snapshot(&root);
        let a = make(&t, &root, "safe.purr", Level::Normal, None, true);
        let plain = make(&t, &root, "plain.purr", Level::Normal, None, false);
        let (with, without) = (fs::metadata(&a).unwrap().len(), fs::metadata(&plain).unwrap().len());
        assert!(with > without && with < without * 108 / 100 + 8192, "repair data: {} vs {}", with, without);
        assert!(list(&a, None).unwrap().purr.unwrap().repair_data);
        assert!(!list(&plain, None).unwrap().purr.unwrap().repair_data);
        assert!(verify(&a, &Quiet::default()).unwrap().clean);
        // damage: a zeroed stretch in the data, a flipped bit in the first bytes, one in the table of contents
        let good = fs::read(&a).unwrap();
        let mut bad = good.clone();
        bad[3] ^= 0x80;
        let hole = (without as usize / 100).max(2000);
        bad[50_000..50_000 + hole].fill(0);
        let toc = without as usize - 200;
        bad[toc] ^= 0x01;
        let broken = t.p("broken.purr");
        fs::write(&broken, &bad).unwrap();
        let err = unpack(&t, &broken, "no", None).unwrap_err();
        assert!(matches!(&err, Error::Damaged(m) if m.contains("repair data")), "{:?}", err);
        assert!(repairable(&broken));
        let fixed = t.p("fixed.purr");
        let rep = repair(&broken, &fixed, &Quiet::default()).unwrap();
        assert!(rep.repaired && rep.damaged >= 2);
        assert_eq!(fs::read(&fixed).unwrap(), good, "the repaired copy is byte for byte the original archive");
        assert_eq!(fs::read(&broken).unwrap(), bad, "the damaged file itself was left alone");
        let out = unpack(&t, &fixed, "yes", None).unwrap();
        assert_eq!(snapshot(&out.join("stuff")), want);
        // an archive without repair data says so
        assert!(repair(&plain, &t.p("x.purr"), &Quiet::default()).is_err());
        // locked archives repair without the password
        let locked = make(&t, &root, "locked.purr", Level::Fast, Some("pw"), true);
        let mut lb = fs::read(&locked).unwrap();
        let lgood = lb.clone();
        lb[1234] ^= 0xFF;
        fs::write(t.p("locked-bad.purr"), &lb).unwrap();
        assert!(repair(&t.p("locked-bad.purr"), &t.p("locked-fixed.purr"), &Quiet::default()).unwrap().repaired);
        assert_eq!(fs::read(t.p("locked-fixed.purr")).unwrap(), lgood);
    }

    #[test]
    fn gather_and_write_skip_what_cannot_be_read() {
        let t = Tmp::new();
        let root = tree(&t, false);
        let mut skipped = Vec::new();
        let mut items = gather(&[root.clone()], &mut skipped).unwrap();
        items.push(Item { src: t.p("does-not-exist.txt"), name: "stuff/ghost.txt".into(), is_dir: false, size: 10 });
        let a = t.p("a.purr");
        let opts = PurrOptions { level: Level::Fast, password: None, repair: false, low_memory: true };
        write_purr(&items, &a, &opts, &Quiet::default(), &mut skipped).unwrap();
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].0, "stuff/ghost.txt");
        let out = unpack(&t, &a, "out", None).unwrap();
        assert_eq!(snapshot(&out.join("stuff")), snapshot(&root));
    }
}
