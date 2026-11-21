fn main() {
    // The UnRAR build asks for libpthread; on Android that lives inside libc, so give the
    // linker an empty stand-in archive.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        std::fs::write(out.join("libpthread.a"), b"!<arch>\n").unwrap();
        println!("cargo:rustc-link-search=native={}", out.display());
        // UnRAR is C++: bundle the C++ runtime (libc++_shared.so from the NDK) inside the app.
        if let Ok(ndk) = std::env::var("NDK_HOME").or_else(|_| std::env::var("ANDROID_NDK_HOME")) {
            let triple = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
                "aarch64" => "aarch64-linux-android",
                "arm" => "arm-linux-androideabi",
                "x86" => "i686-linux-android",
                _ => "x86_64-linux-android",
            };
            let prebuilt = std::path::Path::new(&ndk).join("toolchains/llvm/prebuilt");
            if let Ok(rd) = std::fs::read_dir(&prebuilt) {
                for host in rd.flatten() {
                    let lib = host.path().join("sysroot/usr/lib").join(triple);
                    let so = lib.join("libc++_shared.so");
                    if so.exists() {
                        // Copy just this library next to the stub (that folder's libc.a must not be linked).
                        let _ = std::fs::copy(&so, out.join("libc++_shared.so"));
                        // (Tauri's Android build packs libc++_shared.so into the app by itself.)
                    }
                }
            }
        }
        println!("cargo:rustc-link-lib=dylib=c++_shared");
    }
    tauri_build::build()
}
