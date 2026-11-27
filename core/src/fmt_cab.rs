//! Windows cabinet (.cab) reading.

use crate::safe::Sink;
use crate::{unix_from_parts, Entry, Error, Result, Wanted};
use crate::split::Source as File;
use std::io::BufReader;
use std::path::Path;

fn open(path: &Path) -> Result<cab::Cabinet<BufReader<File>>> {
    cab::Cabinet::new(BufReader::new(crate::split::open(path)?)).map_err(|e| Error::Damaged(e.to_string()))
}

fn names(c: &cab::Cabinet<BufReader<File>>) -> Vec<(String, u64, Option<i64>)> {
    let mut v = Vec::new();
    for folder in c.folder_entries() {
        for f in folder.file_entries() {
            let t = f.datetime().map(|d| {
                unix_from_parts(d.year() as i64, u8::from(d.month()) as u32, d.day() as u32, d.hour() as u32, d.minute() as u32, d.second() as u32)
            });
            v.push((f.name().to_string(), f.uncompressed_size() as u64, t));
        }
    }
    v
}

pub fn list(path: &Path) -> Result<Vec<Entry>> {
    let c = open(path)?;
    Ok(names(&c)
        .into_iter()
        .enumerate()
        .map(|(i, (n, size, t))| Entry {
            index: i,
            path: n.replace('\\', "/"),
            size,
            packed: None,
            is_dir: false,
            encrypted: false,
            modified: t,
            special: false,
        })
        .collect())
}

pub fn extract(path: &Path, wanted: &Wanted, sink: &mut Sink) -> Result<()> {
    let mut c = open(path)?;
    let list = names(&c);
    for (i, (name, _, t)) in list.iter().enumerate() {
        if !wanted.has(i) {
            continue;
        }
        let mut r = c.read_file(name).map_err(|e| Error::Damaged(e.to_string()))?;
        sink.file(i, &name.replace('\\', "/"), &mut r, *t)?;
    }
    Ok(())
}
