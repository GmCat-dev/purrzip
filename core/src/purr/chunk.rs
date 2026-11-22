//! Cutting a stream into content-defined chunks (FastCDC). Where the cuts fall depends only on
//! the bytes around them, so the same data produces the same chunks wherever it appears, which
//! is what lets Purrzip store repeated data once.
//!
//! The chunking is not part of the file format: an archive records each chunk's length, so a
//! reader never needs to repeat this.

use crate::{Error, Result};
use std::io::Read;

pub const MIN: usize = 16 * 1024;
pub const AVG: usize = 64 * 1024;
pub const MAX: usize = 256 * 1024;
const BUF: usize = 4 * MAX;

pub struct Chunker {
    buf: Vec<u8>,
}

impl Default for Chunker {
    fn default() -> Self {
        Chunker { buf: vec![0u8; BUF] }
    }
}

impl Chunker {
    /// Feed `reader` to the end, calling `each` with every chunk in order. Returns the byte count.
    pub fn run(&mut self, reader: &mut dyn Read, each: &mut dyn FnMut(&[u8]) -> Result<()>) -> Result<u64> {
        let mut len = 0usize;
        let mut eof = false;
        let mut total = 0u64;
        loop {
            while !eof && len < BUF {
                match reader.read(&mut self.buf[len..]) {
                    Ok(0) => eof = true,
                    Ok(n) => len += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(Error::Io(e)),
                }
            }
            if len == 0 {
                return Ok(total);
            }
            let mut used = 0usize;
            for c in fastcdc::v2020::FastCDC::new(&self.buf[..len], MIN, AVG, MAX) {
                let end = c.offset + c.length;
                // Without the end of the stream in view, the last cut may only be the buffer's edge:
                // leave it for the next round.
                if !eof && end == len {
                    break;
                }
                each(&self.buf[c.offset..end])?;
                used = end;
            }
            total += used as u64;
            self.buf.copy_within(used..len, 0);
            len -= used;
            if eof && len == 0 {
                return Ok(total);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn cuts(data: &[u8]) -> Vec<usize> {
        let mut out = Vec::new();
        let mut c = Chunker::default();
        let n = c.run(&mut &data[..], &mut |ch| { out.push(ch.len()); Ok(()) }).unwrap();
        assert_eq!(n as usize, data.len());
        assert_eq!(out.iter().sum::<usize>(), data.len());
        out
    }

    #[test]
    fn sizes_are_sane() {
        assert!(cuts(b"").is_empty());
        assert_eq!(cuts(b"tiny"), vec![4]);
        assert_eq!(cuts(&noise(MIN, 1)), vec![MIN]);
        let big = cuts(&noise(5 << 20, 2));
        assert!(big.len() > 20 && big.len() < 300, "{} chunks", big.len());
        assert!(big[..big.len() - 1].iter().all(|&l| (MIN..=MAX).contains(&l)));
    }

    #[test]
    fn same_data_same_cuts_whatever_the_reads_look_like() {
        let data = noise(3 << 20, 3);
        let whole = cuts(&data);
        // a reader that hands out odd-sized pieces
        struct Drip<'a>(&'a [u8], usize);
        impl Read for Drip<'_> {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                self.1 = self.1 % 4093 + 1;
                let n = b.len().min(self.0.len()).min(self.1);
                b[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        let mut out = Vec::new();
        Chunker::default().run(&mut Drip(&data, 7), &mut |ch| { out.push(ch.len()); Ok(()) }).unwrap();
        assert_eq!(out, whole);
    }

    #[test]
    fn shifted_copy_shares_most_chunks() {
        // the same 2 MiB placed after different prefixes: cuts realign, so chunks repeat
        let body = noise(2 << 20, 4);
        let a = [noise(1000, 5), body.clone()].concat();
        let b = [noise(77777, 6), body.clone()].concat();
        let hashes = |d: &[u8]| {
            let mut v = Vec::new();
            Chunker::default().run(&mut &d[..], &mut |ch| { v.push(blake3::hash(ch)); Ok(()) }).unwrap();
            v
        };
        let (ha, hb) = (hashes(&a), hashes(&b));
        let shared = ha.iter().filter(|h| hb.contains(h)).count();
        assert!(shared * 10 >= ha.len() * 8, "only {} of {} chunks shared", shared, ha.len());
    }
}
