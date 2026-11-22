//! Lossless JPEG shrinking with Lepton (the format Dropbox designed for storing photos,
//! here through Microsoft's memory-safe Rust port). A JPEG goes in, a smaller Lepton stream is
//! stored, and unpacking gives back the original file byte for byte.
//!
//! Safety net: at packing time every picture is shrunk, turned back, and compared with the
//! original. If anything differs, or the gain is too small, the picture is stored unchanged.

use crate::{Error, Result};
use std::io::Cursor;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Pictures above these sizes are stored unchanged (the shrinker's memory grows with the picture).
pub const MAX_JPEG_BYTES: u64 = 48 << 20;
pub const MAX_JPEG_PIXELS: u64 = 64_000_000;
pub const MAX_JPEG_PIXELS_LOW_MEM: u64 = 30_000_000;
/// Shrinking must save at least this share, or it isn't worth the unpacking time.
const MIN_GAIN: f64 = 0.03;

fn features() -> lepton_jpeg::EnabledFeatures {
    let mut f = lepton_jpeg::EnabledFeatures::compat_lepton_vector_write();
    f.max_jpeg_file_size = MAX_JPEG_BYTES as u32;
    f
}

/// Width x height from the picture's frame header, if it's a JPEG we can read.
pub fn pixels(data: &[u8]) -> Option<u64> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            return None;
        }
        let m = data[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        if m == 0xD8 || (0xD0..=0xD7).contains(&m) || m == 0x01 {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if len < 2 {
            return None;
        }
        // SOF0..SOF15 except DHT (C4), JPG (C8) and DAC (CC)
        if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
            if i + 9 > data.len() {
                return None;
            }
            let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u64;
            let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u64;
            return Some(w * h);
        }
        if m == 0xDA || m == 0xD9 {
            return None;
        }
        i += 2 + len;
    }
    None
}

/// Shrink a JPEG. Returns None when it should be stored unchanged instead.
pub fn shrink(data: &[u8], max_pixels: u64) -> Option<Vec<u8>> {
    if data.len() as u64 > MAX_JPEG_BYTES || data.len() < 512 {
        return None;
    }
    match pixels(data) {
        Some(p) if p > 0 && p <= max_pixels => {}
        _ => return None,
    }
    let out = catch_unwind(AssertUnwindSafe(|| {
        let mut out = Vec::with_capacity(data.len());
        lepton_jpeg::encode_lepton(&mut Cursor::new(data), &mut Cursor::new(&mut out), &features(), &lepton_jpeg::SingleThreadPool::default()).ok()?;
        Some(out)
    }))
    .ok()??;
    if out.len() as f64 > data.len() as f64 * (1.0 - MIN_GAIN) {
        return None;
    }
    // The safety net: it must come back exactly.
    match restore(&out, data.len() as u64) {
        Ok(back) if back == data => Some(out),
        _ => None,
    }
}

/// Turn a stored Lepton stream back into the original JPEG.
pub fn restore(blob: &[u8], expect_len: u64) -> Result<Vec<u8>> {
    let r = catch_unwind(AssertUnwindSafe(|| {
        let mut out = Vec::with_capacity(expect_len.min(MAX_JPEG_BYTES * 2) as usize);
        lepton_jpeg::decode_lepton(&mut Cursor::new(blob), &mut out, &features(), &lepton_jpeg::SingleThreadPool::default()).map(|_| out)
    }));
    match r {
        Ok(Ok(out)) if out.len() as u64 == expect_len => Ok(out),
        Ok(Ok(_)) => Err(Error::Damaged("a shrunk picture didn't come back at its original size".into())),
        Ok(Err(e)) => Err(Error::Damaged(format!("a shrunk picture couldn't be restored ({})", e))),
        Err(_) => Err(Error::Damaged("a shrunk picture couldn't be restored".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_a_jpeg() {
        assert_eq!(pixels(b"hello world"), None);
        assert!(shrink(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 16, 1, 2, 3], 1 << 40).is_none());
        assert!(shrink(&vec![0xFFu8; 4096], 1 << 40).is_none());
        assert!(restore(b"not lepton at all, definitely", 100).is_err());
    }

    #[test]
    fn reads_frame_size() {
        // SOI, APP0 (len 4), SOF0 with height 0x0102 and width 0x0304
        let d = [0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 9, 9, 0xFF, 0xC0, 0, 11, 8, 1, 2, 3, 4, 3, 0, 0, 0];
        assert_eq!(pixels(&d), Some(0x0102 * 0x0304));
    }
}
