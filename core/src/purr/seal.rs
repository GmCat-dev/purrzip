//! Password protection for .purr archives.
//!
//! - The password (Unicode NFC, UTF-8) goes through Argon2id (memory-hard) with a random salt.
//! - Data is sealed with XChaCha20-Poly1305 in the STREAM construction (64 KiB segments): every
//!   segment is authenticated, segments can't be reordered or dropped, and a blob can't be cut short.
//! - Each sealed blob starts with its own random 19-byte nonce prefix.
//! - The header carries a 16-byte check so a wrong password is told apart from a damaged file.

use super::wire::Kdf;
use crate::{Error, Result};
use aead_stream::{DecryptorBE32, EncryptorBE32};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use std::io::{self, Read};
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroize;

pub const SEGMENT: usize = 64 * 1024;
pub const TAG: usize = 16;
pub const NONCE: usize = 19;

/// Default cost: 64 MiB, 3 passes, 1 lane (RFC 9106's memory-constrained recommendation; about
/// 0.15 s on a laptop, around a second on a phone).
pub const DEFAULT_M_KIB: u32 = 64 * 1024;
pub const DEFAULT_T: u32 = 3;

pub struct Key {
    enc: [u8; 32],
    check: [u8; 32],
}
impl Drop for Key {
    fn drop(&mut self) {
        self.enc.zeroize();
        self.check.zeroize();
    }
}

fn rng(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|_| Error::Other("This device couldn't make random numbers for the lock.".into()))
}

pub fn new_kdf() -> Result<Kdf> {
    let mut salt = [0u8; 16];
    rng(&mut salt)?;
    Ok(Kdf { m_kib: DEFAULT_M_KIB, t: DEFAULT_T, p: 1, salt })
}

pub fn derive(password: &str, kdf: &Kdf) -> Result<Key> {
    let mut pw: String = password.nfc().collect();
    let params = argon2::Params::new(kdf.m_kib, kdf.t, kdf.p, Some(32)).map_err(|e| Error::Other(format!("lock settings: {}", e)))?;
    let a2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut master = [0u8; 32];
    let r = a2.hash_password_into(pw.as_bytes(), &kdf.salt, &mut master);
    pw.zeroize();
    r.map_err(|e| Error::Other(format!("lock: {}", e)))?;
    let key = Key {
        enc: blake3::derive_key("Purrzip .purr v1 2026-11 data encryption", &master),
        check: blake3::derive_key("Purrzip .purr v1 2026-11 password check", &master),
    };
    master.zeroize();
    Ok(key)
}

impl Key {
    /// The value stored in the header to recognise the right password.
    pub fn header_check(&self, header_prefix: &[u8]) -> [u8; 16] {
        let h = blake3::keyed_hash(&self.check, header_prefix);
        let mut out = [0u8; 16];
        out.copy_from_slice(&h.as_bytes()[..16]);
        out
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&self.enc).into())
    }

    /// Seal a whole blob held in memory.
    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0u8; NONCE];
        rng(&mut nonce)?;
        let segments = plain.len().div_ceil(SEGMENT).max(1);
        let mut out = Vec::with_capacity(NONCE + plain.len() + segments * TAG);
        out.extend_from_slice(&nonce);
        let mut enc = EncryptorBE32::from_aead(self.cipher(), (&nonce).into());
        let mut buf: Vec<u8> = Vec::with_capacity(SEGMENT + TAG);
        let mut parts = plain.chunks(SEGMENT).peekable();
        if parts.peek().is_none() {
            enc.encrypt_last_in_place(b"", &mut buf).map_err(|_| Error::Other("Couldn't lock the data.".into()))?;
            out.extend_from_slice(&buf);
            return Ok(out);
        }
        while let Some(part) = parts.next() {
            buf.clear();
            buf.extend_from_slice(part);
            if parts.peek().is_some() {
                enc.encrypt_next_in_place(b"", &mut buf).map_err(|_| Error::Other("Couldn't lock the data.".into()))?;
                out.extend_from_slice(&buf);
            } else {
                enc.encrypt_last_in_place(b"", &mut buf).map_err(|_| Error::Other("Couldn't lock the data.".into()))?;
                out.extend_from_slice(&buf);
                break;
            }
        }
        Ok(out)
    }

    /// Open a sealed blob of `sealed_len` bytes as a stream.
    pub fn unseal<R: Read>(&self, inner: R, sealed_len: u64) -> Unsealer<R> {
        Unsealer { inner, left: sealed_len, dec: None, cipher: Some(self.cipher()), buf: Vec::with_capacity(SEGMENT + TAG), pos: 0, done: false }
    }
}

/// How many bytes a sealed blob takes for a given plain length.
#[allow(dead_code)]
pub fn sealed_len(plain_len: u64) -> u64 {
    NONCE as u64 + plain_len + (plain_len.div_ceil(SEGMENT as u64).max(1)) * TAG as u64
}

pub struct Unsealer<R: Read> {
    inner: R,
    left: u64,
    dec: Option<DecryptorBE32<XChaCha20Poly1305>>,
    cipher: Option<XChaCha20Poly1305>,
    buf: Vec<u8>,
    pos: usize,
    done: bool,
}

fn bad() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "locked data doesn't check out (damaged or changed)")
}

impl<R: Read> Unsealer<R> {
    fn fill(&mut self) -> io::Result<()> {
        if self.dec.is_none() {
            if self.left < (NONCE + TAG) as u64 {
                return Err(bad());
            }
            let mut nonce = [0u8; NONCE];
            self.inner.read_exact(&mut nonce)?;
            self.left -= NONCE as u64;
            let cipher = self.cipher.take().ok_or_else(bad)?;
            self.dec = Some(DecryptorBE32::from_aead(cipher, (&nonce).into()));
        }
        let take = self.left.min((SEGMENT + TAG) as u64) as usize;
        if take < TAG {
            return Err(bad());
        }
        self.buf.resize(take, 0);
        self.inner.read_exact(&mut self.buf)?;
        self.left -= take as u64;
        self.pos = 0;
        if self.left == 0 {
            let dec = self.dec.take().ok_or_else(bad)?;
            dec.decrypt_last_in_place(b"", &mut self.buf).map_err(|_| bad())?;
            self.done = true;
        } else {
            self.dec.as_mut().ok_or_else(bad)?.decrypt_next_in_place(b"", &mut self.buf).map_err(|_| bad())?;
        }
        Ok(())
    }
}

impl<R: Read> Read for Unsealer<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.pos == self.buf.len() {
            if self.done {
                return Ok(0);
            }
            self.fill()?;
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> Key {
        // tiny cost so tests are quick
        derive("pässword ☕", &Kdf { m_kib: 64, t: 1, p: 1, salt: [3; 16] }).unwrap()
    }
    fn open(k: &Key, sealed: &[u8]) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        k.unseal(sealed, sealed.len() as u64).read_to_end(&mut out)?;
        Ok(out)
    }

    #[test]
    fn round_trips_at_every_boundary() {
        let k = key();
        for n in [0usize, 1, 100, SEGMENT - 1, SEGMENT, SEGMENT + 1, 2 * SEGMENT, 3 * SEGMENT + 77] {
            let plain: Vec<u8> = (0..n).map(|i| (i * 31 % 251) as u8).collect();
            let sealed = k.seal(&plain).unwrap();
            assert_eq!(sealed.len() as u64, sealed_len(n as u64), "size for {}", n);
            assert_eq!(open(&k, &sealed).unwrap(), plain, "round trip for {}", n);
        }
    }

    #[test]
    fn detects_tampering_truncation_and_reordering() {
        let k = key();
        let plain = vec![7u8; 3 * SEGMENT + 10];
        let sealed = k.seal(&plain).unwrap();
        for i in [0usize, 5, NONCE, NONCE + 100, sealed.len() / 2, sealed.len() - 1] {
            let mut m = sealed.clone();
            m[i] ^= 0x40;
            assert!(open(&k, &m).is_err(), "flip at {}", i);
        }
        // cut after a whole segment: must not pass as a shorter blob
        let cut = NONCE + SEGMENT + TAG;
        assert!(open(&k, &sealed[..cut]).is_err());
        assert!(open(&k, &sealed[..sealed.len() - 1]).is_err());
        // swap the first two segments
        let mut sw = sealed.clone();
        let seg = SEGMENT + TAG;
        let (a, b) = (NONCE, NONCE + seg);
        let first: Vec<u8> = sw[a..a + seg].to_vec();
        sw.copy_within(b..b + seg, a);
        sw[b..b + seg].copy_from_slice(&first);
        assert!(open(&k, &sw).is_err());
    }

    #[test]
    fn password_and_salt_matter() {
        let kdf = Kdf { m_kib: 64, t: 1, p: 1, salt: [3; 16] };
        let a = derive("meow", &kdf).unwrap();
        let b = derive("meow ", &kdf).unwrap();
        let c = derive("meow", &Kdf { salt: [4; 16], ..kdf.clone() }).unwrap();
        assert_ne!(a.header_check(b"h"), b.header_check(b"h"));
        assert_ne!(a.header_check(b"h"), c.header_check(b"h"));
        assert_eq!(a.header_check(b"h"), derive("meow", &kdf).unwrap().header_check(b"h"));
        let sealed = a.seal(b"secret").unwrap();
        assert!(open(&b, &sealed).is_err());
        // the same letters typed in a different Unicode form give the same key
        assert_eq!(derive("caf\u{e9}", &kdf).unwrap().header_check(b"h"), derive("cafe\u{301}", &kdf).unwrap().header_check(b"h"));
    }

    #[test]
    fn two_seals_differ() {
        let k = key();
        assert_ne!(k.seal(b"same").unwrap(), k.seal(b"same").unwrap());
    }
}
