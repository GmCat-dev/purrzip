# Security

Archives are a classic way to attack a computer, so Purrzip treats every archive as hostile. If you find a way to make
Purrzip write outside the folder you picked, overwrite an existing file, create a link, crash in a way that looks
exploitable, or run code, please report it privately:

**GitHub → this repository → Security → "Report a vulnerability".**

Please include the archive (or a script that makes it) and steps to reproduce. We'll reply as soon as we can and credit
you if you'd like.

What Purrzip already does (see `core/src/safe.rs`): names with `..` are refused, absolute paths and drive letters are
kept inside the chosen folder, Windows-reserved names and characters are renamed, files are created with "create new"
so nothing on disk is overwritten, links and special files from archives are never created, and each folder is checked
to be inside the destination before writing.

Out of scope: archives that are simply very large (Purrzip shows progress and has a Stop button), and weak protection
of archives made by other tools (for example old ZipCrypto passwords).
