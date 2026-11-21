//! Makes .zst and .cab test files (no command-line tools for these here).
use std::io::Write;
fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("dir"));
    let data = b"Purrzip cab and zstd test data. ".repeat(5000);
    // .zst single file and .tar.zst
    let hello = std::fs::read(dir.join("hello.txt")).unwrap();
    std::fs::write(dir.join("hello.txt.zst"), zstd::encode_all(&hello[..], 3).unwrap()).unwrap();
    let tar = std::fs::read(dir.join("plain.tar")).unwrap();
    std::fs::write(dir.join("tree.tar.zst"), zstd::encode_all(&tar[..], 3).unwrap()).unwrap();
    // .cab with two files, MSZIP
    let mut b = cab::CabinetBuilder::new();
    {
        let f = b.add_folder(cab::CompressionType::MsZip);
        f.add_file("readme.txt");
        f.add_file("sub\\inner.bin");
    }
    let out = std::fs::File::create(dir.join("test.cab")).unwrap();
    let mut w = b.build(out).unwrap();
    while let Some(mut fw) = w.next_file().unwrap() {
        if fw.file_name() == "readme.txt" { fw.write_all(&data).unwrap(); } else { fw.write_all(&[7u8; 70000]).unwrap(); }
    }
    w.finish().unwrap();
}
