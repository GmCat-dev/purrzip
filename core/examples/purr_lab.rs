//! Lab bench used while designing the .purr format: measures the candidate codecs on real data.
//! Not part of the app. Usage:
//!   purr_lab codecs <file> [pack_mib]      compare zstd / xz / ppmd on one file
//!   purr_lab jpeg <files...>               lepton round trip: sizes and speeds
//!   purr_lab misc                          hashing, chunking, Reed-Solomon, Argon2 speeds
use std::io::{Cursor, Read, Write};
use std::time::Instant;

fn mbps(bytes: usize, secs: f64) -> f64 {
    bytes as f64 / 1048576.0 / secs.max(1e-9)
}

fn zstd_c(data: &[u8], level: i32, wlog: u32, ldm: bool) -> Vec<u8> {
    let mut e = zstd::stream::Encoder::new(Vec::new(), level).unwrap();
    e.window_log(wlog).unwrap();
    e.long_distance_matching(ldm).unwrap();
    e.include_checksum(false).unwrap();
    e.set_pledged_src_size(Some(data.len() as u64)).unwrap();
    e.write_all(data).unwrap();
    e.finish().unwrap()
}
fn zstd_d(c: &[u8], n: usize) -> Vec<u8> {
    let mut d = zstd::stream::Decoder::new(c).unwrap();
    d.window_log_max(31).unwrap();
    let mut out = Vec::with_capacity(n);
    d.read_to_end(&mut out).unwrap();
    out
}
fn xz_c(data: &[u8], preset: u32, dict: u32, extreme: bool, bcj: Option<lzma_rust2::FilterType>) -> Vec<u8> {
    let mut o = lzma_rust2::XzOptions::with_preset(preset);
    o.lzma_options.dict_size = dict;
    if extreme {
        o.lzma_options.nice_len = 273;
    }
    o.set_check_sum_type(lzma_rust2::CheckType::None);
    if let Some(f) = bcj {
        o.prepend_pre_filter(f, 0);
    }
    let mut w = lzma_rust2::XzWriter::new(Vec::new(), o).unwrap();
    w.write_all(data).unwrap();
    w.finish().unwrap()
}
fn xz_d(c: &[u8], n: usize) -> Vec<u8> {
    let mut r = lzma_rust2::XzReader::new(c, false);
    let mut out = Vec::with_capacity(n);
    r.read_to_end(&mut out).unwrap();
    out
}
fn ppmd_c(data: &[u8], order: u32, mem: u32) -> Vec<u8> {
    let mut e = ppmd_rust::Ppmd7Encoder::new(Vec::new(), order, mem).unwrap();
    e.write_all(data).unwrap();
    e.finish(false).unwrap()
}
fn ppmd_d(c: &[u8], n: usize, order: u32, mem: u32) -> Vec<u8> {
    let mut d = ppmd_rust::Ppmd7Decoder::new(c, order, mem).unwrap();
    let mut out = vec![0u8; n];
    d.read_exact(&mut out).unwrap();
    out
}

fn run(name: &str, data: &[u8], enc: &dyn Fn(&[u8]) -> Vec<u8>, dec: &dyn Fn(&[u8], usize) -> Vec<u8>) {
    let t = Instant::now();
    let c = enc(data);
    let te = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let d = dec(&c, data.len());
    let td = t.elapsed().as_secs_f64();
    assert!(d == data, "{} round trip failed", name);
    println!(
        "{:<28} {:>10} B  {:>6.2}%  enc {:>7.1} MB/s  dec {:>7.1} MB/s",
        name,
        c.len(),
        c.len() as f64 * 100.0 / data.len() as f64,
        mbps(data.len(), te),
        mbps(data.len(), td)
    );
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(|s| s.as_str()) {
        Some("codecs") => {
            let mut data = std::fs::read(&a[1]).unwrap();
            let pack = a.get(2).and_then(|s| s.parse::<usize>().ok()).unwrap_or(64) << 20;
            data.truncate(pack);
            let only = a.get(3).cloned().unwrap_or_default();
            let want = |n: &str| only.is_empty() || only.split(',').any(|p| n.starts_with(p));
            println!("{} ({} MiB)", a[1], data.len() >> 20);
            let wl = (usize::BITS - (data.len() - 1).leading_zeros()).clamp(20, 27);
            for (n, l, ldm) in [("zstd-3", 3, false), ("zstd-9", 9, false), ("zstd-15", 15, false), ("zstd-19", 19, false), ("zstd-22", 22, false), ("zstd-22-ldm", 22, true)] {
                if want(n) {
                    run(n, &data, &|d| zstd_c(d, l, wl, ldm), &zstd_d);
                }
            }
            for (n, p, dict, ex) in [("xz-3 d4M", 3u32, 4u32 << 20, false), ("xz-6 d8M", 6, 8 << 20, false), ("xz-6 d16M", 6, 16 << 20, false), ("xz-9 d64M", 9, 64 << 20, false), ("xz-9e d64M", 9, 64 << 20, true)] {
                if want(n) {
                    run(n, &data, &|d| xz_c(d, p, dict.min(1 << wl), ex, None), &xz_d);
                }
            }
            if want("xz-bcj") {
                run("xz-bcj x86 9 d64M", &data, &|d| xz_c(d, 9, 64 << 20, false, Some(lzma_rust2::FilterType::BcjX86)), &xz_d);
            }
            for (n, o, m) in [("ppmd o6 m64", 6u32, 64u32 << 20), ("ppmd o8 m64", 8, 64 << 20), ("ppmd o8 m128", 8, 128 << 20), ("ppmd o12 m192", 12, 192 << 20)] {
                if want(n) {
                    run(n, &data, &|d| ppmd_c(d, o, m), &|c, len| ppmd_d(c, len, o, m));
                }
            }
        }
        Some("xztune") => {
            // purr_lab xztune <file> <pack_mib> preset:dict_mib:nice:depth:lc ...
            let mut data = std::fs::read(&a[1]).unwrap();
            let pack = a[2].parse::<usize>().unwrap() << 20;
            data.truncate(pack);
            println!("{} ({} MiB)", a[1], data.len() >> 20);
            for spec in &a[3..] {
                let v: Vec<u32> = spec.split(':').map(|x| x.parse().unwrap()).collect();
                let enc = |d: &[u8]| {
                    let mut o = lzma_rust2::XzOptions::with_preset(v[0]);
                    o.lzma_options.dict_size = v[1] << 20;
                    o.lzma_options.nice_len = v[2];
                    o.lzma_options.depth_limit = v[3] as i32;
                    o.lzma_options.lc = v[4];
                    if v.len() > 6 {
                        o.lzma_options.lp = v[5];
                        o.lzma_options.pb = v[6];
                    }
                    if v.len() > 7 && v[7] == 1 {
                        o.prepend_pre_filter(lzma_rust2::FilterType::BcjX86, 0);
                    }
                    o.set_check_sum_type(lzma_rust2::CheckType::None);
                    let mut w = lzma_rust2::XzWriter::new(Vec::new(), o).unwrap();
                    w.write_all(d).unwrap();
                    w.finish().unwrap()
                };
                run(&format!("xz {}", spec), &data, &enc, &xz_d);
            }
        }
        Some("ppmd") => {
            let mut data = std::fs::read(&a[1]).unwrap();
            let pack = a[2].parse::<usize>().unwrap() << 20;
            data.truncate(pack);
            println!("{} ({} MiB)", a[1], data.len() >> 20);
            for spec in &a[3..] {
                let (o, m) = spec.split_once(':').unwrap();
                let (o, m): (u32, u32) = (o.parse().unwrap(), m.parse().unwrap());
                run(&format!("ppmd o{} m{}", o, m), &data, &|d| ppmd_c(d, o, m << 20), &|c, len| ppmd_d(c, len, o, m << 20));
            }
        }
        Some("jpeg") => {
            let feats = lepton_jpeg::EnabledFeatures::compat_lepton_vector_write();
            let (mut tin, mut tout) = (0usize, 0usize);
            for p in &a[1..] {
                let data = std::fs::read(p).unwrap();
                let t = Instant::now();
                let mut out = Vec::new();
                let r = lepton_jpeg::encode_lepton(&mut Cursor::new(&data), &mut Cursor::new(&mut out), &feats, &lepton_jpeg::SingleThreadPool::default());
                let te = t.elapsed().as_secs_f64();
                match r {
                    Ok(_) => {
                        let t = Instant::now();
                        let mut back = Vec::new();
                        lepton_jpeg::decode_lepton(&mut Cursor::new(&out), &mut back, &feats, &lepton_jpeg::SingleThreadPool::default()).unwrap();
                        let td = t.elapsed().as_secs_f64();
                        assert!(back == data, "lepton round trip mismatch");
                        tin += data.len();
                        tout += out.len();
                        println!("{:>9} -> {:>9}  {:>5.1}% smaller  enc {:>6.1} MB/s  dec {:>6.1} MB/s  {}", data.len(), out.len(), 100.0 - out.len() as f64 * 100.0 / data.len() as f64, mbps(data.len(), te), mbps(data.len(), td), p.rsplit('/').next().unwrap());
                    }
                    Err(e) => println!("{:>9}    not supported: {}  {}", data.len(), e, p.rsplit('/').next().unwrap()),
                }
            }
            println!("TOTAL {} -> {}  {:.1}% smaller", tin, tout, 100.0 - tout as f64 * 100.0 / tin.max(1) as f64);
        }
        Some("misc") => {
            let n = 256usize << 20;
            let mut data = vec![0u8; n];
            let mut x = 0x9E3779B97F4A7C15u64;
            for b in data.chunks_mut(8) {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                b.copy_from_slice(&x.to_le_bytes()[..b.len()]);
            }
            let t = Instant::now();
            let h = blake3::hash(&data);
            println!("blake3            {:>8.0} MB/s  {}", mbps(n, t.elapsed().as_secs_f64()), &h.to_hex()[..8]);
            let t = Instant::now();
            let c = crc32fast::hash(&data);
            println!("crc32             {:>8.0} MB/s  {:08x}", mbps(n, t.elapsed().as_secs_f64()), c);
            let t = Instant::now();
            let chunks: Vec<_> = fastcdc::v2020::FastCDC::new(&data, 16 << 10, 64 << 10, 256 << 10).collect();
            println!("fastcdc 64K       {:>8.0} MB/s  {} chunks", mbps(n, t.elapsed().as_secs_f64()), chunks.len());
            let t = Instant::now();
            let mut hs = 0u8;
            for c in &chunks {
                hs ^= blake3::hash(&data[c.offset..c.offset + c.length]).as_bytes()[0];
            }
            println!("blake3 per chunk  {:>8.0} MB/s  {}", mbps(n, t.elapsed().as_secs_f64()), hs);
            for (k, p, s) in [(8192usize, 256usize, 4096usize), (1024, 32, 32768), (64, 2, 524288)] {
                let t = Instant::now();
                let mut enc = reed_solomon_simd::ReedSolomonEncoder::new(k, p, s).unwrap();
                for i in 0..k {
                    enc.add_original_shard(&data[i * s..(i + 1) * s]).unwrap();
                }
                let r = enc.encode().unwrap();
                let cnt = r.recovery_iter().count();
                println!("reed-solomon {:>5}+{:<4} x {:>6} B  {:>8.0} MB/s  ({} parity)", k, p, s, mbps(k * s, t.elapsed().as_secs_f64()), cnt);
            }
            for (m, tc) in [(19456u32, 2u32), (65536, 3)] {
                let t = Instant::now();
                let params = argon2::Params::new(m, tc, 1, Some(32)).unwrap();
                let a2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
                let mut out = [0u8; 32];
                a2.hash_password_into(b"correct horse", b"0123456789abcdef", &mut out).unwrap();
                println!("argon2id m={} MiB t={}  {:.2} s", m / 1024, tc, t.elapsed().as_secs_f64());
            }
        }
        _ => eprintln!("usage: purr_lab codecs|jpeg|misc ..."),
    }
}
