# Third-party code

Purrzip is built on open-source libraries. Their licences are kept with their source code
(fetched by Cargo from crates.io when you build). This is the list for the app as of this release.

Highlights:

- **zip** (MIT): ZIP reading and writing, including AES
- **sevenz-rust2**, **lzma-rust2** (Apache-2.0): 7z, LZMA and XZ
- **unrar** / **unrar_sys** (MIT or Apache-2.0) wrapping the **UnRAR** library by Alexander Roshal (UnRAR licence; see [LICENSE-EXCEPTION.md](LICENSE-EXCEPTION.md)). Used only to open RAR archives.
- **tar**, **flate2**, **bzip2** (MIT or Apache-2.0), **zstd** (BSD-3-Clause / MIT), **cab** (MIT)
- **Tauri** (MIT or Apache-2.0): the app shell on Windows, macOS, Linux and Android
- **tiny_http** (MIT or Apache-2.0): only in the local test server

All 516 crates, grouped by licence:

**MIT OR Apache-2.0** (224): aes 0.9.4, android_system_properties 0.1.6, anyhow 1.0.104, async-broadcast 0.7.2, async-recursion 1.2.0, async-trait 0.1.92, base64 0.21.7, base64 0.23.1, bitflags 2.13.2, block-buffer 0.10.4, block-buffer 0.12.1, block-padding 0.4.2, bumpalo 3.20.4, bzip2 0.6.1, camino 1.2.6, cargo-platform 0.1.9, cbc 0.2.1, cc 1.11.3, cfg-expr 0.15.8, cfg-if 1.0.5, chrono 0.4.45, cipher 0.5.2, cookie 0.18.2, core-foundation 0.10.1, core-foundation-sys 0.8.7, core-graphics 0.25.0, core-graphics-types 0.2.0, cpubits 0.1.1, cpufeatures 0.2.17, cpufeatures 0.3.1, crc32fast 1.5.5, crossbeam-channel 0.5.17, crossbeam-utils 0.8.23, crypto-common 0.1.7, crypto-common 0.2.2, deranged 0.5.9, digest 0.10.7, digest 0.11.3, dirs 7.0.0, dirs-sys 0.5.0, displaydoc 0.2.7, dtoa 1.0.11, dyn-clone 1.0.20, embed_plist 1.2.2, enumflags2 0.7.12, enumflags2_derive 0.7.12, erased-serde 0.4.11, errno 0.3.15, fdeflate 0.3.7, field-offset 0.3.6, find-msvc-tools 0.1.15, flate2 1.1.11, form_urlencoded 1.2.3, futures-channel 0.3.34, futures-core 0.3.34, futures-executor 0.3.34, futures-io 0.3.34, futures-macro 0.3.34, futures-sink 0.3.34, futures-task 0.3.34, futures-util 0.3.34, getrandom 0.3.4, getrandom 0.4.3, glob 0.3.4, hashbrown 0.12.3, hashbrown 0.17.1, heck 0.4.1, heck 0.5.0, hermit-abi 0.5.3, hex 0.4.3, hmac 0.13.0, html5ever 0.39.0, html5ever 0.40.1, http 1.5.0, httparse 1.10.1, hybrid-array 0.4.15, iana-time-zone 0.1.65, iana-time-zone-haiku 0.1.2, idna 1.1.0, inout 0.2.6, ipnet 2.12.2, itoa 1.0.19, jni-sys 0.3.1, jni-sys 0.4.1, jni-sys-macros 0.4.1, jobserver 0.1.35, js-sys 0.3.107, jsonptr 0.7.1, keyboard-types 0.8.3, libc 0.2.191, lock_api 0.4.14, log 0.4.35, lzxd 0.2.7, markup5ever 0.39.0, markup5ever 0.40.0, mime 0.3.18, ndk 0.9.0, ndk-context 0.1.1, ndk-sys 0.6.0+11769913, num-conv 0.2.2, num-traits 0.2.19, once_cell 1.21.4, ordered-stream 0.2.0, parking_lot 0.12.5, parking_lot_core 0.9.12, pbkdf2 0.13.0, percent-encoding 2.3.2, piper 0.2.5, pkg-config 0.3.34, png 0.18.1, powerfmt 0.2.1, proc-macro-crate 1.3.1, proc-macro-crate 2.0.2, proc-macro-crate 3.5.0, proc-macro-error 1.0.4, proc-macro-error-attr 1.0.4, proc-macro2 1.0.107, quote 1.0.47, ref-cast 1.0.27, ref-cast-impl 1.0.27, regex 1.13.2, regex-automata 0.4.19, regex-syntax 0.8.12, reqwest 0.13.5, rustc_version 0.4.1, rustversion 1.0.23, scopeguard 1.2.0, semver 1.0.28, serde 1.0.229, serde-untagged 0.1.9, serde_core 1.0.229, serde_derive 1.0.229, serde_derive_internals 0.29.1, serde_json 1.0.154, serde_repr 0.1.21, serde_spanned 0.6.9, serde_spanned 1.1.3, serde_with 3.25.0, serde_with_macros 3.25.0, serialize-to-javascript 0.1.2, serialize-to-javascript-impl 0.1.2, servo_arc 0.4.3, servo_arc 0.5.0, sha1 0.11.0, sha2 0.10.9, sha2 0.11.1, shlex 2.0.1, signal-hook-registry 1.4.8, siphasher 1.0.4, smallvec 1.16.6, socket2 0.6.5, softbuffer 0.4.8, stable_deref_trait 1.2.1, string_cache 0.11.0, string_cache 0.9.0, string_cache_codegen 0.11.2, string_cache_codegen 0.6.1, swift-rs 1.0.9, syn 1.0.109, syn 2.0.119, syn 3.0.10, system-deps 6.2.2, tao-macros 0.1.4, tar 0.4.46, tempfile 3.27.0, tendril 0.5.1, thiserror 1.0.69, thiserror 2.0.21, thiserror-impl 1.0.69, thiserror-impl 2.0.21, time 0.3.55, time-core 0.1.9, time-macros 0.2.32, toml 0.8.2, toml 1.2.2+spec-1.1.0, toml_datetime 0.6.3, toml_datetime 1.1.3+spec-1.1.0, toml_edit 0.19.15, toml_edit 0.20.2, toml_edit 0.25.20+spec-1.1.0, toml_parser 1.1.6+spec-1.1.0, toml_writer 1.2.0+spec-1.1.0, tray-icon 0.25.1, typed-path 0.13.0, typeid 1.0.3, typenum 1.20.1, unicode-segmentation 1.13.3, unrar 0.5.8, unrar_sys 0.5.8, url 2.5.9, wasm-bindgen 0.2.130, wasm-bindgen-futures 0.4.80, wasm-bindgen-macro 0.2.130, wasm-bindgen-macro-support 0.2.130, wasm-bindgen-shared 0.2.130, wasm-streams 0.5.0, web-sys 0.3.107, web-time 1.1.0, web_atoms 0.2.6, web_atoms 0.3.0, widestring 1.2.1, windows 0.62.2, windows-collections 0.3.2, windows-core 0.62.2, windows-future 0.3.2, windows-implement 0.60.2, windows-interface 0.59.3, windows-link 0.2.1, windows-numerics 0.3.1, windows-result 0.4.1, windows-strings 0.5.1, windows-sys 0.45.0, windows-sys 0.61.2, windows-targets 0.42.2, windows-threading 0.2.1, windows-version 0.1.7, windows_aarch64_gnullvm 0.42.2, windows_aarch64_msvc 0.42.2, windows_i686_gnu 0.42.2, windows_i686_msvc 0.42.2, windows_x86_64_gnu 0.42.2, windows_x86_64_gnullvm 0.42.2, windows_x86_64_msvc 0.42.2, xattr 1.7.0

**MIT** (124): atk 0.18.2, atk-sys 0.18.2, block2 0.6.2, bytes 1.12.2, cab 0.6.0, cairo-rs 0.18.5, cairo-sys-rs 0.18.2, cargo_metadata 0.19.2, cfb 0.14.0, cfg_aliases 0.2.2, combine 4.6.9, darling 0.24.1, darling_core 0.24.1, darling_macro 0.24.1, deflate64 0.1.12, derive_more 2.1.1, derive_more-impl 2.1.1, dlopen2 0.8.2, dlopen2_derive 0.4.3, dom_query 0.28.0, dom_query 0.29.0, embed-resource 3.0.12, endi 1.1.1, gdk 0.18.2, gdk-pixbuf 0.18.5, gdk-pixbuf-sys 0.18.0, gdk-sys 0.18.2, gdkwayland-sys 0.18.2, gdkx11 0.18.2, gdkx11-sys 0.18.2, generic-array 0.14.7, gio 0.18.4, gio-sys 0.18.1, glib 0.18.5, glib-macros 0.18.5, glib-sys 0.18.1, gobject-sys 0.18.0, gtk 0.18.2, gtk-sys 0.18.2, gtk3-macros 0.18.2, http-body 1.1.0, http-body-util 0.1.5, http-range 0.1.6, hyper 1.12.0, hyper-util 0.1.21, ico 0.6.0, infer 0.22.0, is-docker 0.2.0, is-wsl 0.4.0, javascriptcore-rs 1.1.2, javascriptcore-rs-sys 1.1.1, libredox 0.1.25, memoffset 0.9.1, mio 1.2.5, new_debug_unreachable 1.0.6, nix 0.32.0, objc2 0.6.5, objc2-encode 4.1.0, objc2-foundation 0.3.2, open 5.4.5, pango 0.18.3, pango-sys 0.18.0, phf 0.13.1, phf 0.14.0, phf_codegen 0.13.1, phf_codegen 0.14.0, phf_generator 0.13.1, phf_generator 0.14.0, phf_macros 0.13.1, phf_macros 0.14.0, phf_shared 0.13.1, phf_shared 0.14.0, plist 1.10.4, precomputed-hash 0.1.1, quick-xml 0.42.0, redox_syscall 0.5.18, redox_users 0.5.3, rfd 0.17.2, schemars 0.8.22, schemars 0.9.0, schemars 1.2.3, schemars_derive 0.8.22, simd-adler32 0.3.10, slab 0.4.12, soup3 0.5.0, soup3-sys 0.5.0, strsim 0.11.1, synstructure 0.14.0, tauri-winres 0.3.7, tokio 1.54.1, tokio-util 0.7.20, tower 0.5.3, tower-http 0.6.11, tower-layer 0.3.3, tower-service 0.3.3, tracing 0.1.44, tracing-attributes 0.1.31, tracing-core 0.1.36, try-lock 0.2.5, uds_windows 1.2.1, urlpattern 0.6.0, version-compare 0.2.1, vswhom 0.1.0, vswhom-sys 0.1.3, want 0.3.2, webkit2gtk 2.0.2, webkit2gtk-sys 2.0.2, webview2-com 0.39.1, webview2-com-macros 0.8.1, webview2-com-sys 0.39.1, winnow 0.5.40, winnow 1.0.5, winreg 0.56.0, x11 2.21.0, x11-dl 2.21.0, zbus 5.19.0, zbus_macros 5.19.0, zbus_names 4.3.5, zcheapstr 1.4.0, zip 9.8.1, zmij 1.0.23, zvariant 5.15.1, zvariant_derive 5.15.1, zvariant_utils 4.2.0

**Apache-2.0 OR MIT** (57): async-channel 2.5.0, async-executor 1.14.1, async-io 2.6.0, async-lock 3.5.0, async-process 2.5.0, async-signal 0.2.14, async-task 4.7.1, atomic-waker 1.1.2, autocfg 1.5.1, bit-set 0.11.1, bit-set 0.8.0, bit-vec 0.10.2, bit-vec 0.8.0, blocking 1.7.0, cargo_toml 1.0.1, cmov 0.5.4, concurrent-queue 2.5.0, const-oid 0.10.3, ctor 1.0.14, ctutils 0.4.8, equivalent 1.0.3, event-listener 5.4.2, event-listener-strategy 0.5.4, fastrand 2.5.0, futures-lite 2.6.1, idna_adapter 1.2.2, indexmap 1.9.3, indexmap 2.14.2, libappindicator 0.9.0, libappindicator-sys 0.9.0, link-section 0.19.4, muda 0.20.0, parking 2.2.1, pin-project-lite 0.2.18, polling 3.11.0, portable-atomic 1.15.0, portable-atomic-util 0.2.8, rustc-hash 2.1.3, tauri 2.12.7, tauri-build 2.7.6, tauri-codegen 2.7.6, tauri-macros 2.7.6, tauri-plugin 2.7.4, tauri-plugin-dialog 2.8.4, tauri-plugin-fs 2.7.1, tauri-plugin-opener 2.7.3, tauri-plugin-single-instance 2.5.5, tauri-runtime 2.12.4, tauri-runtime-wry 2.12.5, tauri-utils 2.10.4, utf8_iter 1.1.0, utf_types 1.0.0, uuid 1.29.3, window-vibrancy 0.8.1, wry 0.57.3, zeroize 1.9.2, zeroize_derive 1.5.0

**Unicode-3.0** (18): icu_collections 2.4.0, icu_locale_core 2.4.0, icu_normalizer 2.4.0, icu_normalizer_data 2.4.0, icu_properties 2.4.0, icu_properties_data 2.4.0, icu_provider 2.4.0, litemap 0.8.3, potential_utf 0.1.6, tinystr 0.8.5, writeable 0.6.4, yoke 0.8.4, yoke-derive 0.8.4, zerofrom 0.1.8, zerofrom-derive 0.1.8, zerotrie 0.2.5, zerovec 0.11.9, zerovec-derive 0.11.7

**Zlib OR Apache-2.0 OR MIT** (17): bytemuck 1.25.2, dispatch2 0.3.1, objc2-app-kit 0.3.2, objc2-cloud-kit 0.3.2, objc2-core-data 0.3.2, objc2-core-foundation 0.3.2, objc2-core-graphics 0.3.2, objc2-core-image 0.3.2, objc2-core-location 0.3.2, objc2-core-text 0.3.2, objc2-exception-helper 0.1.1, objc2-io-surface 0.3.2, objc2-quartz-core 0.3.2, objc2-ui-kit 0.3.2, objc2-user-notifications 0.3.2, objc2-web-kit 0.3.2, tinyvec 1.13.3

**Unlicense OR MIT** (14): aho-corasick 1.1.5, byteorder 1.5.0, jiff 0.2.46, jiff 1.0.2, jiff-core 0.1.8, jiff-core 1.0.1, jiff-static 0.2.46, jiff-static 1.0.2, jiff-tzdb 0.1.10, jiff-tzdb 1.0.0, jiff-tzdb-platform 0.1.3, jiff-tzdb-platform 1.0.0, memchr 2.8.3, winapi-util 0.1.11

**MIT/Apache-2.0** (13): bitflags 1.3.2, bs58 0.5.1, filetime 0.2.30, foreign-types 0.5.0, foreign-types-macros 0.2.4, foreign-types-shared 0.3.1, ident_case 1.0.1, jni 0.21.1, json-patch 4.2.0, version_check 0.9.5, winapi 0.3.9, winapi-i686-pc-windows-gnu 0.4.0, winapi-x86_64-pc-windows-gnu 0.4.0

**MPL-2.0** (7): cssparser 0.37.0, cssparser 0.38.0, cssparser-macros 0.7.1, dtoa-short 0.3.5, option-ext 0.2.0, selectors 0.38.0, selectors 0.41.0

**Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT** (5): linux-raw-sys 0.12.1, rustix 1.1.6, wasi 0.11.1+wasi-snapshot-preview1, wasip2 1.0.4+wasi-0.2.12, wit-bindgen 0.57.1

**Apache-2.0** (5): lzma-rust2 0.25.0, sevenz-rust2 0.25.0, sync_wrapper 1.0.2, tao 0.37.3, zopfli 0.8.4

**BSD-3-Clause** (4): alloc-no-stdlib 3.0.0, alloc-stdlib 0.3.0, zstd 0.14.4, zstd-safe 8.3.0

**Apache-2.0/MIT** (3): cesu8 1.1.0, dbus 0.9.12, libdbus-sys 0.2.7

**CC0-1.0 OR MIT-0 OR Apache-2.0** (2): constant_time_eq 0.6.1, dunce 1.0.5

**Unlicense/MIT** (2): same-file 1.0.6, walkdir 2.5.1

**MIT OR Zlib OR Apache-2.0** (2): miniz_oxide 0.8.9, miniz_oxide 0.9.1

**MIT OR Apache-2.0 OR Zlib** (2): raw-window-handle 0.6.2, static_vcruntime 3.0.0

**BSD-3-Clause OR MIT OR Apache-2.0** (2): num_enum 0.7.6, num_enum_derive 0.7.6

**Zlib** (2): foldhash 0.2.0, zlib-rs 0.6.8

**MIT OR Apache-2.0 OR LGPL-2.1-or-later** (2): r-efi 5.3.0, r-efi 6.0.0

**Apache-2.0 WITH LLVM-exception** (1): target-lexicon 0.12.16

**bzip2-1.0.6** (1): libbz2-rs-sys 0.2.7

**BSD-3-Clause AND BSD-2-Clause** (1): zstd-sys 2.1.3+zstd.1.5.7

**0BSD OR MIT OR Apache-2.0** (1): adler2 2.0.1

**CC0-1.0 OR MIT-0** (1): ppmd-rust 1.5.2

**BSD-3-Clause AND MIT** (1): brotli 9.1.3

**Apache-2.0 AND MIT** (1): dpi 0.1.2

**BSD-3-Clause/MIT** (1): brotli-decompressor 6.3.2

**ISC** (1): libloading 0.7.4

**(MIT OR Apache-2.0) AND Unicode-3.0** (1): unicode-ident 1.0.27

**Apache-2.0 / MIT** (1): fnv 1.0.7

The ISO 9660 reader in `core/src/fmt_iso.rs` and the rest of Purrzip are GmCat's own code (AGPL-3.0-or-later).
