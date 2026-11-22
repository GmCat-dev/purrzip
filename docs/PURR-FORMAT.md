# The .purr archive format, version 1

Purrzip's own archive format. This document is the reference: anyone can write a reader from it.
The code lives in `core/src/purr/`.

**Status:** version 1.0, in testing. It may still change until Purrzip's public launch; from then
on every Purrzip will keep reading every .purr ever made.

## What it is for

A .purr holds files and folders, like a .zip or .7z. It differs in five ways:

1. **Repeats are stored once.** Files are cut into chunks at content-defined points. A chunk that
   already exists anywhere in the archive is only pointed at, however far apart the copies are.
2. **Each kind of file gets a method that suits it.** Text and data use LZMA2 (XZ) with a large
   dictionary, programs go through a machine-code filter first, already-packed files get a fast
   pass that only looks for long repeats, and "fast" archives use Zstandard.
3. **JPEG photos shrink by about 20% without changing.** They are stored as Lepton streams and
   come back byte for byte.
4. **A password hides names as well as contents**, with authenticated encryption.
5. **Optional repair data** (Reed-Solomon, about 3% extra) lets a damaged archive be fixed.

Design rules: every squeezing method is an existing, documented stream format with other
implementations (Zstandard RFC 8878, XZ, PPMd7/variant H, Lepton). Every byte that is read back
is checked. Memory to unpack does not grow with the archive: it is bounded by the largest window
a pack uses (128 MiB as written by Purrzip) plus the photos being restored at that moment.
Measured with Purrzip 0.2: about 80 MB for folders of text and programs, and 240 to 270 MB for a
324 MB folder holding 2,600 photos (phone and PC).

## Layout

All numbers are little-endian. A *varint* is an unsigned LEB128 number (at most 10 bytes).

```
header        96 bytes
packs         back to back, each an opaque blob
index         the table of contents (one blob)
footer        64 bytes
repair data   optional
```

### Header (96 bytes)

| offset | size | field |
|---|---|---|
| 0 | 8 | magic `89 50 55 52 52 0D 0A 1A` (`\x89PURR\r\n\x1a`) |
| 8 | 1 | format major version (1) |
| 9 | 1 | format minor version (0) |
| 10 | 2 | flags; bit 0 = locked with a password. Other bits must be 0 |
| 16 | 1 | key derivation: 0 none, 1 Argon2id v1.3 |
| 20 | 4 | Argon2 memory in KiB |
| 24 | 4 | Argon2 passes |
| 28 | 4 | Argon2 lanes |
| 32 | 16 | salt |
| 48 | 16 | password check |
| 92 | 4 | CRC-32 of bytes 0..92 |

Unlisted bytes are zero. A reader refuses a major version it does not know.

### Packs

A pack is the concatenation of one or more chunks, squeezed as a whole with one method, then
(in locked archives) sealed. Packs carry no header of their own; the index says where each one
is and how to decode it.

| method | stream |
|---|---|
| 0 | stored as is |
| 1 | one Zstandard frame |
| 2 | one XZ stream (LZMA2, optionally behind a BCJ filter for x86, ARM64 or ARM code); no XZ integrity check field |
| 3 | PPMd7 (variant H, 7z range coder), preceded by 3 bytes: order (1), model memory in MiB (2). No end marker; the length is known. Purrzip 0.2 never writes this, but reads it |

A reader decodes a pack only as far as it needs and reads exactly the pack's unpacked length.
Reader limits: Zstandard window up to 2^27, XZ dictionary up to 256 MiB, PPMd memory up to
256 MiB, unpacked pack length up to 1 GiB.

### Index

The index blob is: the serialized index, squeezed as one Zstandard frame, then sealed if the
archive is locked. The serialized index is a sequence of sections:

```
section = tag (varint)  length (varint)  body
```

Sections with tags below 64 must be understood (an unknown one means "made by a newer
version"); tags from 64 up may be skipped. Sections appear in the order below.

**Tag 1, meta:** needed reader major (1 byte), minor (1 byte), creation time in seconds since
1970 (varint), creator name length (varint) and bytes (UTF-8).

**Tag 2, packs:** count (varint), then for each pack in file order: gap before it (varint, bytes
between the previous pack's end and this one; the first pack is measured from offset 96), stored
length (varint), unpacked length (varint), method (1 byte), number of chunks (varint).

**Tag 3, chunks:** count (varint, must equal the sum of the packs' chunk counts), then every
chunk's length (varint), in pack order, then every chunk's check. Chunk offsets inside a pack are
the running sum of lengths, which must end exactly at the pack's unpacked length. The check is
the first 4 bytes of the chunk's BLAKE3 hash for chunks shorter than 16384 bytes, the first 8
bytes otherwise. Chunk ids are positions in this list.

**Tag 4, files:** count (varint); length of the names block (varint) and the block (every path
followed by a zero byte; `/` separates folders; UTF-8); one flag byte per entry (bit 0 folder,
bit 1 has a transform, bit 2 has permission bits); for each non-folder its size (varint); for
every entry its modification time in milliseconds since 1970, as a zig-zag varint of the
difference to the previous entry; for each entry with bit 2 its Unix permission bits (varint);
for each entry with bit 1 the transform number (1 byte) and the first 8 bytes of the BLAKE3 hash
of the original file; then for each non-folder its extents: count (varint) and that many pairs
(first chunk id as a zig-zag varint of the difference to the end of the previous extent in the
whole table; number of chunks as a varint).

A file's content is its extents' chunks in order. For a plain file their lengths must add up to
its size. With transform 1 (Lepton) the chunks hold a Lepton stream and the file's size is the
size of the JPEG that comes out, which must hash to the stored check.

**Tag 64, statistics (optional):** total size of all files, bytes found to be repeats, JPEG bytes
in, Lepton bytes out, number of pictures shrunk (five varints; newer writers may append more).

A reader must check everything: counts fit in the bytes present, references stay in range, sums
match, packs lie inside the pack area. Paths are untrusted: `..` is refused, absolute paths and
drive letters are kept inside the destination, and links are never created.

### Footer (64 bytes)

| offset | size | field |
|---|---|---|
| 0 | 8 | magic `PURREND1` |
| 8 | 8 | offset of the index blob |
| 16 | 8 | stored length of the index blob |
| 24 | 8 | length of the serialized index |
| 32 | 1 | index method (1 = Zstandard) |
| 40 | 16 | first 16 bytes of the BLAKE3 hash of the serialized index; all zero in locked archives |
| 60 | 4 | CRC-32 of bytes 0..60 |

The index blob ends exactly where the footer begins.

## Password lock

- The password is normalized to Unicode NFC and taken as UTF-8.
- `master = Argon2id(password, salt, memory, passes, lanes)`, 32 bytes. Purrzip writes 64 MiB,
  3 passes, 1 lane. Readers accept 8 KiB to 1 GiB, 1 to 64 passes, 1 to 16 lanes.
- `data key = BLAKE3 derive_key("Purrzip .purr v1 2026-11 data encryption", master)`
- `check key = BLAKE3 derive_key("Purrzip .purr v1 2026-11 password check", master)`
- The header's password check is the first 16 bytes of `BLAKE3 keyed_hash(check key, header[0..48])`.
  It tells a wrong password apart from a damaged file.
- A sealed blob is a 19-byte random nonce prefix followed by segments. Each segment is up to
  65536 bytes of data plus a 16-byte tag, encrypted with XChaCha20-Poly1305 under the data key,
  nonce = prefix, then a 32-bit big-endian segment counter, then one byte that is 1 for the last
  segment and 0 otherwise (the STREAM construction). An empty blob is one empty last segment.
- Every pack and the index are sealed. Nothing about the files is readable without the password
  except the total size.

## Repair data

Follows the footer. Everything before it is the *body*.

```
head (64)   checksum table   parity sectors   checksum table again   tail (64)
```

Head and tail hold the same fields: magic (`PURRRECH` / `PURRRECT`), version 1 (4 bytes),
sector size S (4 bytes), body length (8), number of data sectors N (8), number of groups G (8),
parity sectors per group P (8), zeros, CRC-32 of the first 60 bytes at offset 60.

The body is cut into N sectors of S bytes (the last one padded with zeros). Sector *i* belongs to
group *i mod G*. Each group's sectors, in order, are the original shards of a Reed-Solomon code
over GF(2^16) (the Leopard-RS construction as implemented by `reed-solomon-simd` 3.x), with P
recovery shards. The parity area holds group 0's P recovery shards, then group 1's, and so on.

The checksum table has one CRC-32 per data sector, then one per parity sector, in blocks of 4096
entries, each block followed by the CRC-32 of the block. It is stored twice.

To repair: find head or tail, take each table block from whichever copy checks out, mark every
sector whose CRC does not match as lost, and rebuild each group that has no more lost data
sectors than good parity sectors. Because neighbouring sectors belong to different groups, one
damaged stretch of up to about P·G·S bytes (about 3% of the archive) can be rebuilt.

Purrzip chooses S between 1 KiB and 512 KiB so that there are about 50,000 sectors, groups of at
most 32 MiB of data, and P = 3% of a group (at least 2). Repair needs no password.

## What the writer does (not part of the format)

Chunking uses FastCDC (16 KiB minimum, 64 KiB average, 256 KiB maximum); a shrunk photo is one
chunk. Files are sorted by kind, then by type, then by path, so similar files share a pack.
Packs hold up to 16, 32 or 128 MiB depending on the level; packs of photos and already-packed
files hold up to 128 MiB at every level, so far-apart repeats are still found (phones use smaller
packs to save memory). Every photo is shrunk, restored and compared with the original before its
Lepton stream is accepted. A reader needs none of this.
