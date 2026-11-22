//! The squeezing methods a pack can use, and how files are sorted into kinds.
//!
//! Every method here is an existing, documented stream format with other implementations:
//! Zstandard frames (RFC 8878), XZ streams (LZMA2, optionally behind a BCJ filter) and PPMd7
//! (variant H, as in 7z). A pack records only the method number; each stream describes itself.

use super::wire::{CODEC_PPMD7, CODEC_STORE, CODEC_XZ, CODEC_ZSTD};
use crate::{Error, Result};
use std::io::{Read, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bcj {
    X86,
    Arm64,
    Arm,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Recipe {
    Store,
    /// `long` turns on Zstandard's long-distance matching: cheap, and it finds repeats far apart
    /// (used for already-packed files, which often share long identical stretches).
    Zstd { level: i32, long: bool },
    Xz { preset: u32, dict: u32, extreme: bool, bcj: Option<Bcj> },
    Ppmd { order: u32, mem_mib: u32 },
}

impl Recipe {
    pub fn codec(&self) -> u8 {
        match self {
            Recipe::Store => CODEC_STORE,
            Recipe::Zstd { .. } => CODEC_ZSTD,
            Recipe::Xz { .. } => CODEC_XZ,
            Recipe::Ppmd { .. } => CODEC_PPMD7,
        }
    }
}

/// Largest values a reader accepts (a hostile archive must not make us allocate gigabytes).
pub const MAX_PPMD_MEM_MIB: u32 = 256;
const XZ_MEM_LIMIT_KIB: u32 = 320 * 1024; // dictionary up to 256 MiB
const ZSTD_WINDOW_LOG_MAX: u32 = 27;

fn io_err(e: std::io::Error) -> Error {
    Error::Io(e)
}

pub fn compress(recipe: &Recipe, raw: &[u8]) -> Result<Vec<u8>> {
    compress_ticking(recipe, raw, &mut |_| true)
}

/// Feed `raw` to a writer in slices, reporting how far along we are. `tick` returns false to stop.
fn feed(w: &mut dyn Write, raw: &[u8], tick: &mut dyn FnMut(usize) -> bool) -> Result<()> {
    let mut done = 0;
    for part in raw.chunks(256 * 1024) {
        w.write_all(part).map_err(io_err)?;
        done += part.len();
        if !tick(done) {
            return Err(Error::Cancelled);
        }
    }
    Ok(())
}

/// Squeeze `raw` with one method. `tick(bytes_done)` is called as it goes and can stop the work.
pub fn compress_ticking(recipe: &Recipe, raw: &[u8], tick: &mut dyn FnMut(usize) -> bool) -> Result<Vec<u8>> {
    match *recipe {
        Recipe::Store => Ok(raw.to_vec()),
        Recipe::Zstd { level, long } => {
            let mut e = zstd::stream::Encoder::new(Vec::with_capacity(raw.len() / 3 + 64), level).map_err(io_err)?;
            if long {
                e.long_distance_matching(true).map_err(io_err)?;
            }
            // Window just big enough for the whole pack (so far-apart repeats are found), never over the reader's limit.
            let wlog = (usize::BITS - raw.len().max(2).saturating_sub(1).leading_zeros()).clamp(17, ZSTD_WINDOW_LOG_MAX - 1);
            e.window_log(wlog).map_err(io_err)?;
            e.include_checksum(false).map_err(io_err)?;
            e.set_pledged_src_size(Some(raw.len() as u64)).map_err(io_err)?;
            feed(&mut e, raw, tick)?;
            e.finish().map_err(io_err)
        }
        Recipe::Xz { preset, dict, extreme, bcj } => {
            let mut o = lzma_rust2::XzOptions::with_preset(preset);
            // A dictionary bigger than the data only wastes memory.
            o.lzma_options.dict_size = dict.min((raw.len().max(4096) as u64).next_power_of_two().min(1 << 30) as u32).max(4096);
            if extreme {
                o.lzma_options.nice_len = 273;
            }
            o.set_check_sum_type(lzma_rust2::CheckType::None); // every chunk already has its own check
            if let Some(b) = bcj {
                let f = match b {
                    Bcj::X86 => lzma_rust2::FilterType::BcjX86,
                    Bcj::Arm64 => lzma_rust2::FilterType::BcjArm64,
                    Bcj::Arm => lzma_rust2::FilterType::BcjArm,
                };
                o.prepend_pre_filter(f, 0);
            }
            let mut w = lzma_rust2::XzWriter::new(Vec::with_capacity(raw.len() / 3 + 64), o).map_err(io_err)?;
            feed(&mut w, raw, tick)?;
            w.finish().map_err(io_err)
        }
        Recipe::Ppmd { order, mem_mib } => {
            let mem_mib = mem_mib.clamp(1, MAX_PPMD_MEM_MIB);
            let mut out = Vec::with_capacity(raw.len() / 3 + 64);
            out.push(order as u8);
            out.extend_from_slice(&(mem_mib as u16).to_le_bytes());
            let mut e = ppmd_rust::Ppmd7Encoder::new(out, order, mem_mib << 20).map_err(|e| Error::Other(format!("ppmd: {}", e)))?;
            feed(&mut e, raw, tick)?;
            e.finish(false).map_err(io_err)
        }
    }
}

/// Wrap a pack's stored bytes in the right decoder. The caller reads exactly the pack's raw length.
pub fn decoder<'a>(codec: u8, mut stored: Box<dyn Read + 'a>) -> Result<Box<dyn Read + 'a>> {
    match codec {
        CODEC_STORE => Ok(stored),
        CODEC_ZSTD => {
            let mut d = zstd::stream::Decoder::new(stored).map_err(io_err)?;
            d.window_log_max(ZSTD_WINDOW_LOG_MAX).map_err(io_err)?;
            Ok(Box::new(d))
        }
        CODEC_XZ => Ok(Box::new(lzma_rust2::XzReader::new_mem_limit(stored, false, XZ_MEM_LIMIT_KIB))),
        CODEC_PPMD7 => {
            let mut h = [0u8; 3];
            stored.read_exact(&mut h).map_err(|_| Error::Damaged("a squeezed block is cut short".into()))?;
            let order = h[0] as u32;
            let mem_mib = u16::from_le_bytes([h[1], h[2]]) as u32;
            if !(ppmd_rust::PPMD7_MIN_ORDER..=ppmd_rust::PPMD7_MAX_ORDER).contains(&order) || mem_mib == 0 || mem_mib > MAX_PPMD_MEM_MIB {
                return Err(Error::Damaged("a squeezed block has impossible settings".into()));
            }
            let d = ppmd_rust::Ppmd7Decoder::new(stored, order, mem_mib << 20).map_err(|e| Error::Damaged(format!("ppmd: {}", e)))?;
            Ok(Box::new(d))
        }
        _ => Err(Error::Unsupported("a .purr that uses a newer squeezing method. Update Purrzip to open it".into())),
    }
}

// ---------- sorting files into kinds ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// JPEG pictures: shrunk with the photo method.
    Photo,
    /// Plain text of any sort (code, logs, documents in text form).
    Text,
    /// Programs: squeezed behind a filter made for machine code.
    Exec(u8),
    /// Other squeezable data.
    Data,
    /// Already squeezed (video, music, zips, most pictures): kept as it is.
    Packed,
}

const PACKED_EXT: &[&str] = &[
    "png", "gif", "webp", "avif", "heic", "heif", "jxl", "jp2", "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "3gp", "mpg",
    "mpeg", "mp3", "m4a", "aac", "ogg", "oga", "opus", "flac", "wma", "zip", "7z", "rar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst",
    "lz4", "lzma", "br", "cab", "apk", "aab", "ipa", "jar", "war", "docx", "xlsx", "pptx", "odt", "ods", "odp", "epub", "woff", "woff2",
    "purr", "dmg", "pkg", "msi", "appimage", "deb", "rpm", "crx", "xpi", "whl", "nupkg", "kmz", "cbz", "cbr",
];
const TEXT_EXT: &[&str] = &[
    "txt", "md", "markdown", "rst", "csv", "tsv", "json", "jsonl", "ndjson", "xml", "html", "htm", "xhtml", "css", "scss", "sass", "less",
    "js", "mjs", "cjs", "jsx", "ts", "tsx", "map", "vue", "svelte", "py", "pyi", "rb", "php", "pl", "pm", "lua", "sh", "bash", "zsh", "fish",
    "bat", "cmd", "ps1", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "m", "mm", "cs", "java", "kt", "kts", "scala", "go", "rs", "swift",
    "dart", "sql", "r", "jl", "hs", "ml", "ex", "exs", "erl", "clj", "lisp", "el", "vim", "tex", "bib", "log", "ini", "cfg", "conf", "toml",
    "yaml", "yml", "properties", "gradle", "cmake", "mk", "make", "svg", "srt", "vtt", "ass", "nfo", "diff", "patch", "po", "pot", "strings",
    "plist", "xaml", "proto", "graphql", "gql", "tf", "nix", "lock", "gitignore", "env", "htaccess", "ics", "vcf", "eml", "rtf", "asm", "s",
];

pub fn ext_of(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name);
    match base.rfind('.') {
        Some(i) if i > 0 && base.len() - i <= 12 => base[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

fn u16le(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}
fn u32le(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

/// Which machine a program file is for, if it is one (Windows PE, Linux ELF, macOS Mach-O).
fn exec_arch(head: &[u8]) -> Option<Bcj> {
    if head.starts_with(b"MZ") {
        let pe = u32le(head, 0x3c)? as usize;
        if head.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        return match u16le(head, pe + 4)? {
            0x8664 | 0x014c => Some(Bcj::X86),
            0xAA64 => Some(Bcj::Arm64),
            0x01c0 | 0x01c4 => Some(Bcj::Arm),
            _ => None,
        };
    }
    if head.starts_with(b"\x7fELF") {
        let big = *head.get(5)? == 2;
        let m = if big { u16::from_be_bytes([*head.get(18)?, *head.get(19)?]) } else { u16le(head, 18)? };
        return match m {
            3 | 62 => Some(Bcj::X86),
            183 => Some(Bcj::Arm64),
            40 => Some(Bcj::Arm),
            _ => None,
        };
    }
    let magic = u32le(head, 0)?;
    if magic == 0xfeedfacf || magic == 0xfeedface {
        return match u32le(head, 4)? {
            0x01000007 | 0x00000007 => Some(Bcj::X86),
            0x0100000c => Some(Bcj::Arm64),
            0x0000000c => Some(Bcj::Arm),
            _ => None,
        };
    }
    None
}

pub fn bcj_of(tag: u8) -> Bcj {
    match tag {
        1 => Bcj::Arm64,
        2 => Bcj::Arm,
        _ => Bcj::X86,
    }
}

/// Decide a file's kind from its name and its first bytes (`head`: up to 256 KiB).
/// Unknown kinds get a quick trial squeeze: if that barely helps, the file is treated as already packed.
pub fn classify(name: &str, size: u64, head: &[u8]) -> Kind {
    if head.len() >= 3 && head[0] == 0xFF && head[1] == 0xD8 && head[2] == 0xFF {
        return Kind::Photo;
    }
    if size == 0 {
        return Kind::Data;
    }
    if let Some(b) = exec_arch(head) {
        return Kind::Exec(match b {
            Bcj::X86 => 0,
            Bcj::Arm64 => 1,
            Bcj::Arm => 2,
        });
    }
    let ext = ext_of(name);
    if PACKED_EXT.contains(&ext.as_str()) {
        return Kind::Packed;
    }
    let known_text = TEXT_EXT.contains(&ext.as_str());
    let sample = &head[..head.len().min(128 * 1024)];
    if sample.len() >= 4096 || !known_text {
        // Trial squeeze of the sample with the fastest method.
        if sample.len() >= 512 {
            if let Ok(c) = zstd::bulk::compress(sample, 1) {
                if c.len() as f64 > sample.len() as f64 * 0.97 {
                    return Kind::Packed;
                }
            }
        }
    }
    if known_text || looks_text(sample) { Kind::Text } else { Kind::Data }
}

fn looks_text(sample: &[u8]) -> bool {
    if sample.is_empty() {
        return false;
    }
    let s = &sample[..sample.len().min(16 * 1024)];
    let odd = s.iter().filter(|&&b| b == 0 || (b < 0x20 && !matches!(b, b'\n' | b'\r' | b'\t' | 0x0c | 0x1b))).count();
    if odd * 200 > s.len() {
        return false;
    }
    // Mostly valid UTF-8 (a cut at the end of the sample is fine).
    match std::str::from_utf8(s) {
        Ok(_) => true,
        Err(e) => e.valid_up_to() + 4 >= s.len() || s.iter().filter(|&&b| b >= 0x80).count() * 20 < s.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(recipe: Recipe, data: &[u8]) {
        let c = compress(&recipe, data).unwrap();
        let mut d = decoder(recipe.codec(), Box::new(&c[..])).unwrap();
        let mut out = vec![0u8; data.len()];
        d.read_exact(&mut out).unwrap();
        assert_eq!(out, data, "{:?}", recipe);
    }

    #[test]
    fn all_methods_round_trip() {
        let mut data = Vec::new();
        for i in 0..40000u32 {
            data.extend_from_slice(format!("line {} of some text that repeats, {}\n", i, i % 97).as_bytes());
        }
        for r in [
            Recipe::Store,
            Recipe::Zstd { level: 3, long: false },
            Recipe::Zstd { level: 1, long: true },
            Recipe::Zstd { level: 19, long: false },
            Recipe::Xz { preset: 6, dict: 16 << 20, extreme: false, bcj: None },
            Recipe::Xz { preset: 9, dict: 64 << 20, extreme: true, bcj: Some(Bcj::X86) },
            Recipe::Xz { preset: 6, dict: 8 << 20, extreme: false, bcj: Some(Bcj::Arm64) },
            Recipe::Ppmd { order: 8, mem_mib: 16 },
        ] {
            round(r, &data);
            round(r, b"");
            round(r, b"x");
        }
    }

    #[test]
    fn kinds() {
        assert_eq!(classify("a.jpg", 10, &[0xFF, 0xD8, 0xFF, 0xE0]), Kind::Photo);
        assert_eq!(classify("renamed.bin", 10, &[0xFF, 0xD8, 0xFF, 0xE1]), Kind::Photo);
        assert_eq!(classify("movie.MP4", 1 << 20, &[0u8; 100]), Kind::Packed);
        assert_eq!(classify("notes.txt", 5, b"hello"), Kind::Text);
        assert_eq!(classify("README", 50, b"plain words here, nothing odd about them at all, ok"), Kind::Text);
        let mut x = 0x9E3779B97F4A7C15u64;
        let noise: Vec<u8> = (0..20000u32)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect();
        assert_eq!(classify("blob.dat", 20000, &noise), Kind::Packed);
        let mut elf = vec![0u8; 64];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[18] = 62;
        assert_eq!(classify("prog", 64, &elf), Kind::Exec(0));
        elf[18] = 183;
        assert_eq!(classify("prog", 64, &elf), Kind::Exec(1));
        assert_eq!(ext_of("a/b/c.TAR"), "tar");
        assert_eq!(ext_of("a/.hidden"), "");
    }

    #[test]
    fn decoder_refuses_absurd_settings() {
        assert!(decoder(CODEC_PPMD7, Box::new(&[200u8, 0xff, 0xff, 0][..])).is_err());
        assert!(decoder(99, Box::new(&[][..])).is_err());
    }
}
