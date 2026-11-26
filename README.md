# Purrzip by GmCat 🗜️🐾

**Open and make archives on your phone and computer.** Free, no ads, no account, no tracking. Your files never leave
your device: there is no server, Purrzip does everything on the device itself.

- **Opens:** `.zip` (including AES and old ZipCrypto passwords), `.7z` (including hidden names), `.rar` (RAR4 and RAR5,
  passwords, multi-part), `.tar`, `.tar.gz`/`.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`, single `.gz`/`.bz2`/`.xz`/`.zst`
  files, `.iso` (ISO 9660 + Joliet) and `.cab`.
- **Makes:** `.zip` and `.7z`, with an optional strong AES-256 password (and hidden file names for `.7z`). Photos,
  videos and other already-squeezed files are stored as they are, which is as small and much faster.
- **Its own format, `.purr`**, for when small matters more than "opens everywhere": repeated files and repeated
  pieces are stored once however far apart they are, JPEG photos shrink by about 20% and come back byte for byte,
  a password hides the names too, and optional repair data lets a damaged archive fix itself.
  The format is documented in [docs/PURR-FORMAT.md](docs/PURR-FORMAT.md).
- **Look before you unzip:** browse folders inside an archive, search, preview pictures, text, video and sound,
  and unzip everything or just the files you tick.
- **Safe with strange archives:** see [SECURITY.md](SECURITY.md). Nothing is ever written outside the folder you pick.
- Works on **Windows, macOS, Linux and Android** (iPhone later).

Purrzip can open RAR files but can't make them: the RAR format belongs to its author.

## Download

**[purr.gmcat.io](https://purr.gmcat.io/)** or the [latest release](https://github.com/GmCat-dev/purrzip/releases/latest):
Windows (`.exe` installer), Mac (`.dmg`, Apple chips and Intel), Linux (`.deb`, AppImage) and Android (`.apk`).
Every download is built by GitHub Actions from this code. The installers aren't code-signed yet, so Windows and
macOS show a warning the first time.

## Where to look

| What | Where |
|---|---|
| Opening every format, making .zip/.7z | [`core/src/`](core/src) (`lib.rs` is the overview, one `fmt_*.rs` per format, `create.rs`) |
| The safety rules for writing files | [`core/src/safe.rs`](core/src/safe.rs) |
| The .purr format: writer, reader, lock, repair data | [`core/src/purr/`](core/src/purr) (`mod.rs` is the overview), spec in [`docs/PURR-FORMAT.md`](docs/PURR-FORMAT.md) |
| Commands the app screen uses (jobs, progress, Stop, previews) | [`service/src/lib.rs`](service/src/lib.rs) |
| The app screen (HTML/CSS/JS, no build step) | [`app/ui/`](app/ui) |
| App shell (Tauri) and Android glue | [`app/src-tauri/`](app/src-tauri), `app/src-tauri/gen/android/app/src/main/java/io/gmcat/purrzip/MainActivity.kt` |
| Command-line version | `core/src/bin/purrzip-cli.rs` |
| Tests | `cargo test -p purrzip-core` (incl. archives made on Linux that every other system must unpack identically), `tests/core_test.py` (formats, passwords, booby-trapped archives, round trips), `tests/ui_test.mjs` (the real screen against the real engine), `tests/bench.py` (sizes and speeds against other tools) |

## Build it yourself

You need [Rust](https://rustup.rs) and Node.js.

```sh
# the engine and its tests
cargo build --release
python3 -I tests/core_test.py target/release/purrzip-cli target/release/examples/make_fixtures \
    ~/.cargo/registry/src/*/unrar-0.5.8/data          # (build the example first: cargo build --release --example make_fixtures)

# the desktop app (Linux needs the Tauri system packages, see tauri.app)
cd app && npm ci && npx tauri build

# try the screen in a browser against the real engine
PZ_ROOT=$HOME PZ_UI=app/ui target/release/purrzip-devserver    # then open http://127.0.0.1:4231
```

Android: `cd app && npx tauri android build --apk` (needs the Android SDK + NDK; set `PURRZIP_KEY_DIR` to sign).

## Licence

Code: [GNU AGPL-3.0-or-later](LICENSE), with an [additional permission](LICENSE-EXCEPTION.md) to combine it with the
UnRAR library. Want to use it closed-source? See the [commercial licence](COMMERCIAL.md), paid in $GMGM.
Third-party code: [NOTICE.md](NOTICE.md).
The Purrzip and GmCat names, the GmCat character and artwork (including the pictures in `app/ui/img/`, the app icons
and `art/`) are not covered by either licence; all rights reserved.

## Reporting a security problem

Please don't open a public issue. See [SECURITY.md](SECURITY.md).
