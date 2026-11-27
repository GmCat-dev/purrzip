#!/usr/bin/env python3
"""Purrzip core self-test. Builds test archives with standard tools, then checks that
Purrzip lists/extracts them correctly, refuses unsafe names, handles passwords, and
that archives Purrzip makes open elsewhere (unzip, Python) and round-trip byte-exact.

Usage: python3 -I core_test.py <path to purrzip-cli> <path to make_fixtures example binary> <rar samples dir>
"""
import hashlib, json, os, random, shutil, stat, subprocess, sys, tarfile, tempfile, zipfile, io

CLI, FIXER, RARDIR = sys.argv[1], sys.argv[2], sys.argv[3]
W = tempfile.mkdtemp(prefix="purrzip-core-test-")
passed, failed = 0, []


def check(name, cond, info=""):
    global passed
    if cond:
        passed += 1
        print("  ok  ", name)
    else:
        failed.append(name)
        print("  FAIL", name, info)


def run(*args):
    p = subprocess.run([CLI, *args], capture_output=True, text=True)
    try:
        return json.loads(p.stdout.strip().splitlines()[-1])
    except Exception:
        return {"ok": False, "code": "crash", "message": p.stdout + p.stderr}


def tree_hashes(root):
    out = {}
    for d, dirs, files in os.walk(root):
        for f in files:
            p = os.path.join(d, f)
            if os.path.islink(p):
                out[os.path.relpath(p, root)] = "LINK"
                continue
            out[os.path.relpath(p, root)] = hashlib.sha256(open(p, "rb").read()).hexdigest()
        for x in dirs:
            out[os.path.relpath(os.path.join(d, x), root) + "/"] = "DIR"
    return out


def fresh(name):
    p = os.path.join(W, "out", name)
    os.makedirs(p)
    return p


# ---------- fixtures ----------
src = os.path.join(W, "src")
tree = os.path.join(src, "tree")
os.makedirs(os.path.join(tree, "sub", "deep"))
os.makedirs(os.path.join(tree, "emptydir"))
rnd = random.Random(7)
open(os.path.join(tree, "a.txt"), "w").write("hello purrzip\n" * 1000)
open(os.path.join(tree, "sub", "b.bin"), "wb").write(os.urandom(3 << 20))
open(os.path.join(tree, "sub", "deep", "c.txt"), "w").write("deep " * 50000)
open(os.path.join(tree, "ünïcødé ☕.txt"), "w", encoding="utf-8").write("unicode name\n")
open(os.path.join(tree, "empty.txt"), "w").close()
open(os.path.join(tree, "photo.jpg"), "wb").write(os.urandom(200000))
EXPECT = tree_hashes(src)  # paths start with tree/

fx = os.path.join(W, "fx")
os.makedirs(fx)
sh = lambda cmd, cwd=src: subprocess.run(cmd, cwd=cwd, shell=True, check=True, capture_output=True)
sh(f"tar -cf {fx}/plain.tar tree")
sh(f"tar -czf {fx}/tree.tar.gz tree")
sh(f"tar -cjf {fx}/tree.tar.bz2 tree")
sh(f"tar -cJf {fx}/tree.tar.xz tree")
sh(f"cp {fx}/tree.tar.gz {fx}/renamed.tgz")
sh(f"zip -qr {fx}/deflate.zip tree")
sh(f"zip -qr0 {fx}/store.zip tree")
sh(f"zip -qr -P s3cret {fx}/zipcrypto.zip tree")
sh(f"genisoimage -quiet -J -R -o {fx}/tree.iso tree")
shutil.copy(os.path.join(tree, "a.txt"), os.path.join(fx, "hello.txt"))
sh("gzip -kN hello.txt && bzip2 -k hello.txt && xz -k hello.txt", cwd=fx)
subprocess.run([FIXER, fx], check=True)
os.rename(os.path.join(fx, "hello.txt"), os.path.join(W, "hello.orig"))

# Booby-trapped zip
with zipfile.ZipFile(os.path.join(fx, "evil.zip"), "w") as z:
    for n in ["../evil.txt", "/abs.txt", "ok/../../evil2.txt", "C:\\win.txt", "con.txt", "dup.txt", "Dup.txt", "fine/ok.txt"]:
        z.writestr(n, "x")
    zi = zipfile.ZipInfo("link")
    zi.create_system = 3
    zi.external_attr = (stat.S_IFLNK | 0o777) << 16
    z.writestr(zi, "/etc/passwd")
# Booby-trapped tar: symlink escape + traversal + hardlink
with tarfile.open(os.path.join(fx, "evil.tar"), "w") as t:
    def add(name, data=b"x", kind=tarfile.REGTYPE, link=""):
        ti = tarfile.TarInfo(name)
        ti.type = kind
        ti.linkname = link
        ti.size = len(data) if kind == tarfile.REGTYPE else 0
        t.addfile(ti, io.BytesIO(data) if kind == tarfile.REGTYPE else None)
    add("../evil.txt")
    add("esc", kind=tarfile.SYMTYPE, link="/tmp")
    add("esc/through-link.txt")
    add("hard", kind=tarfile.LNKTYPE, link="/etc/passwd")
    add("good.txt", b"good")

# ---------- 1. third-party archives open correctly ----------
print("1. Opening archives made by other tools")
for f in ["plain.tar", "tree.tar.gz", "tree.tar.bz2", "tree.tar.xz", "renamed.tgz", "tree.tar.zst", "deflate.zip", "store.zip", "tree.iso"]:
    l = run("list", os.path.join(fx, f))
    check(f"{f}: list", l.get("entries") is not None, l)
    d = fresh(f)
    r = run("extract", os.path.join(fx, f), d)
    got = tree_hashes(d)
    if f == "tree.iso":  # ISO has no top folder of its own; compare under tree/
        got = {("tree/" + k): v for k, v in got.items()}
        exp = {k: v for k, v in EXPECT.items() if k != "tree/"}
    else:
        exp = EXPECT
    missing = {k for k in exp if k not in got or got[k] != exp[k]}
    check(f"{f}: extract byte-exact", r.get("ok") and not missing, (r, sorted(missing)[:5]))

for f, fmt in [("hello.txt.gz", "Gz"), ("hello.txt.bz2", "Bz2"), ("hello.txt.xz", "Xz"), ("hello.txt.zst", "Zst")]:
    l = run("list", os.path.join(fx, f))
    check(f"{f}: detected as {fmt}", l.get("format") == fmt, l.get("format"))
    d = fresh(f)
    run("extract", os.path.join(fx, f), d)
    files = os.listdir(d)
    check(f"{f}: one file, right name + content", files == ["hello.txt"] and open(os.path.join(d, "hello.txt"), "rb").read() == open(os.path.join(W, "hello.orig"), "rb").read(), files)

l = run("list", os.path.join(fx, "test.cab"))
check("test.cab: list", [e["path"] for e in l.get("entries", [])] == ["readme.txt", "sub/inner.bin"], l)
d = fresh("cab")
run("extract", os.path.join(fx, "test.cab"), d)
check("test.cab: extract", os.path.getsize(os.path.join(d, "sub", "inner.bin")) == 70000 and os.path.getsize(os.path.join(d, "readme.txt")) == 160000)

print("2. RAR (sample files that come with the UnRAR library)")
for f in ["solid.rar", "unicode.rar", "utf8.rar", "version.rar", "comment.rar"]:
    p = os.path.join(RARDIR, f)
    d = fresh(f)
    r = run("extract", p, d)
    check(f"{f}: extract", r.get("ok") and r["report"]["files"] >= 1, r)
p = os.path.join(RARDIR, "archive.part1.rar")
l = run("list", p)
check("archive.part1.rar: missing part 2 -> clear message", "part" in l.get("message", ""), l)
p = os.path.join(RARDIR, "crypted.rar")
check("crypted.rar: no password -> asks", run("extract", p, fresh("rc1"))["code"] == "need_password")
check("crypted.rar: wrong password -> says wrong", run("extract", p, fresh("rc2"), "nope").get("code") == "wrong_password")
r = run("extract", p, fresh("rc3"), "unrar")
check("crypted.rar: right password -> opens", r.get("ok"), r)
p = os.path.join(RARDIR, "comment-hpw-password.rar")
check("hidden-names rar: list without password -> asks", run("list", p).get("code") in ("need_password",), run("list", p))
check("hidden-names rar: list with password", run("list", p, "password").get("entries") is not None)

print("3. Passwords on ZIP")
p = os.path.join(fx, "zipcrypto.zip")
check("zipcrypto: no password -> asks", run("extract", p, fresh("zc1"))["code"] == "need_password")
check("zipcrypto: wrong password -> says wrong", run("extract", p, fresh("zc2"), "bad")["code"] in ("wrong_password",))
d = fresh("zc3")
r = run("extract", p, d, "s3cret")
check("zipcrypto: right password -> byte-exact", r.get("ok") and tree_hashes(d) == EXPECT, r)

print("4. Booby-trapped archives")
outside_before = set(os.listdir(os.path.join(W, "out")))
d = fresh("evil-zip")
r = run("extract", os.path.join(fx, "evil.zip"), d)
names = sorted(tree_hashes(d))
check("evil.zip: nothing escapes the folder", set(os.listdir(os.path.join(W, "out"))) == outside_before | {"evil-zip"} and not os.path.exists(os.path.join(W, "evil.txt")))
check("evil.zip: ../ names skipped", sum(1 for s in r["report"]["skipped"] if "unsafe" in s[1]) == 2, r["report"]["skipped"])
check("evil.zip: absolute + drive names kept inside", "abs.txt" in names and "C_/win.txt" in names, names)
check("evil.zip: reserved name renamed", "_con.txt" in names, names)
check("evil.zip: same name twice -> numbered", "dup.txt" in names and "Dup (2).txt" in names, names)
check("evil.zip: link not created", not os.path.lexists(os.path.join(d, "link")), names)
d = fresh("evil-tar")
r = run("extract", os.path.join(fx, "evil.tar"), d)
check("evil.tar: no links made", not any(os.path.islink(os.path.join(dp, f)) for dp, ds, fs in os.walk(d) for f in fs + ds))
check("evil.tar: write through link stayed inside", os.path.isfile(os.path.join(d, "esc", "through-link.txt")) and not os.path.exists("/tmp/through-link.txt"))
check("evil.tar: good file still extracted", open(os.path.join(d, "good.txt")).read() == "good")

print("5. Picking only some files")
l = run("list", os.path.join(fx, "deflate.zip"))
sub = [e["index"] for e in l["entries"] if e["path"] == "tree/sub"]
d = fresh("only")
r = run("extract", os.path.join(fx, "deflate.zip"), d, "--only", ",".join(map(str, sub)))
got = tree_hashes(d)
check("only the picked folder comes out", r.get("ok") and set(k for k in got if not k.endswith("/")) == {"tree/sub/b.bin", "tree/sub/deep/c.txt"}, got)

print("6. Archives Purrzip makes")
for fmt, level, extra in [("zip", "normal", []), ("zip", "store", []), ("zip", "max", []), ("zip", "normal", ["--password", "Paw$ 12"]),
                          ("7z", "normal", []), ("7z", "fast", []), ("7z", "max", []), ("7z", "store", []),
                          ("7z", "normal", ["--password", "Paw$ 12"]), ("7z", "normal", ["--password", "Paw$ 12", "--hide-names"])]:
    tag = f"{fmt}-{level}{'-pw' if extra else ''}{'-hidden' if '--hide-names' in extra else ''}"
    out = os.path.join(W, f"made-{tag}.{fmt}")
    r = run("create", fmt, level, out, *extra, tree)
    check(f"make {tag}", r.get("ok") and os.path.exists(out) and not os.path.exists(out + ".part"), r)
    pw = extra[1:2]
    d = fresh("made-" + tag)
    r2 = run("extract", out, d, *pw)
    check(f"make {tag}: opens byte-exact in Purrzip", r2.get("ok") and tree_hashes(d) == EXPECT, (r2, set(EXPECT) ^ set(tree_hashes(d))))
    if pw:
        check(f"make {tag}: no password -> asks", run("extract", out, fresh("np-" + tag)).get("code") == "need_password")
        check(f"make {tag}: wrong password -> says wrong", run("extract", out, fresh("wp-" + tag), "nope").get("code") == "wrong_password", run("extract", out, fresh("wp2-" + tag), "nope"))
    if "--hide-names" in extra:
        check(f"make {tag}: names hidden without password", run("list", out).get("code") == "need_password")
        check(f"make {tag}: names not in the file", b"deep" not in open(out, "rb").read())
    if fmt == "zip" and not pw:
        u = subprocess.run(["unzip", "-tq", out], capture_output=True, text=True)
        check(f"make {tag}: unzip -t says OK", u.returncode == 0, u.stdout + u.stderr)
        with zipfile.ZipFile(out) as z:
            check(f"make {tag}: Python zipfile reads it", z.testzip() is None)
            info = {i.filename: i for i in z.infolist()}
            if level != "store":
                check(f"make {tag}: photo stored, text squeezed", info["tree/photo.jpg"].compress_type == 0 and info["tree/a.txt"].compress_type == 8)
size_n = os.path.getsize(os.path.join(W, "made-7z-normal.7z"))
size_z = os.path.getsize(os.path.join(W, "made-zip-normal.zip"))
print(f"     sizes: zip normal {size_z:,} B, 7z normal {size_n:,} B")

print("7. Odd inputs")
junk = os.path.join(W, "junk.bin")
open(junk, "wb").write(b"not an archive" * 100)
check("random file -> 'doesn't recognise'", run("list", junk).get("code") == "unknown")
trunc = os.path.join(W, "trunc.zip")
open(trunc, "wb").write(open(os.path.join(fx, "deflate.zip"), "rb").read()[:200000])
r = run("extract", trunc, fresh("trunc"))
check("cut-off zip -> clear error, no crash", r.get("ok") is False and r.get("code") != "crash", r)
trunc7 = os.path.join(W, "trunc.7z")
open(trunc7, "wb").write(open(os.path.join(W, "made-7z-normal.7z"), "rb").read()[:300000])
r = run("extract", trunc7, fresh("trunc7"))
check("cut-off 7z -> clear error, no crash", r.get("ok") is False and r.get("code") != "crash", r)
r = run("test", os.path.join(fx, "tree.tar.xz"))
check("test command", r.get("ok") and r["files"] == 6, r)

print("8. .purr (Purrzip's own format)")
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
psrc = os.path.join(W, "psrc")
ptree = os.path.join(psrc, "stuff")
shutil.copytree(tree, os.path.join(ptree, "tree"))
os.makedirs(os.path.join(ptree, "pics"))
shutil.copy(os.path.join(FIX, "photo.jpg"), os.path.join(ptree, "pics", "beach.jpg"))
shutil.copy(os.path.join(FIX, "photo.jpg"), os.path.join(ptree, "pics", "beach again.jpg"))
shutil.copy(os.path.join(FIX, "photo-progressive.jpg"), os.path.join(ptree, "pics", "sunset.jpg"))
shutil.copytree(os.path.join(ptree, "tree", "sub"), os.path.join(ptree, "backup of sub"))  # 3 MB twin
PEXPECT = tree_hashes(psrc)
sizes = {}
for level in ["store", "fast", "normal", "max"]:
    out = os.path.join(W, f"p-{level}.purr")
    r = run("create", "purr", level, out, ptree)
    check(f"purr {level}: made", r.get("ok") and os.path.exists(out) and not os.path.exists(out + ".part"), r)
    sizes[level] = os.path.getsize(out)
    st = r.get("report", {}).get("purr", {})
    check(f"purr {level}: found the 3 MB twin", st.get("dup_bytes", 0) >= 3 << 20, st)
    if level in ("normal", "max"):
        check(f"purr {level}: shrank 2 photos, found the third as a repeat", st.get("jpeg_count") == 2 and st.get("jpeg_out", 1) < st.get("jpeg_in", 0) * 0.9, st)
    l = run("list", out)
    check(f"purr {level}: lists as Purr", l.get("format") == "Purr" and len(l.get("entries", [])) == len(PEXPECT), (l.get("format"), len(l.get("entries", []))))
    d = fresh(f"p-{level}")
    r2 = run("extract", out, d)
    check(f"purr {level}: unpacks byte-exact", r2.get("ok") and tree_hashes(d) == PEXPECT, (r2, sorted(set(PEXPECT) ^ set(tree_hashes(d)))[:5]))
orig_total = sum(os.path.getsize(os.path.join(dp, f)) for dp, _, fs in os.walk(ptree) for f in fs)
check("purr: twin stored once (archive well under the folder size)", sizes["fast"] < orig_total - (3 << 20) + 200000, (sizes, orig_total))
zref = os.path.join(W, "p-ref.zip")
run("create", "zip", "max", zref, ptree)
check("purr normal is smaller than zip smallest on this folder", sizes["normal"] < os.path.getsize(zref) * 0.6, (sizes["normal"], os.path.getsize(zref)))
l = run("list", os.path.join(W, "p-normal.purr"))
pick = [e["index"] for e in l["entries"] if e["path"] in ("stuff/pics/beach again.jpg", "stuff/tree/a.txt")]
d = fresh("p-only")
r = run("extract", os.path.join(W, "p-normal.purr"), d, "--only", ",".join(map(str, pick)))
got = {k: v for k, v in tree_hashes(d).items() if not k.endswith("/")}
check("purr: only the picked files come out, intact", r.get("ok") and got == {k: PEXPECT[k] for k in ("stuff/pics/beach again.jpg", "stuff/tree/a.txt")}, got)

locked = os.path.join(W, "p-locked.purr")
r = run("create", "purr", "normal", locked, "--password", "Paw$ 12 ☕", ptree)
check("purr locked: made", r.get("ok"), r)
check("purr locked: listing asks for the password", run("list", locked).get("code") == "need_password")
check("purr locked: wrong password says so", run("list", locked, "nope").get("code") == "wrong_password")
blob = open(locked, "rb").read()
check("purr locked: no file name readable in the file", all(n not in blob for n in [b"beach", b"sunset", b"tree", b"stuff", b"a.txt"]))
d = fresh("p-locked")
r = run("extract", locked, d, "Paw$ 12 ☕")
check("purr locked: right password unpacks byte-exact", r.get("ok") and tree_hashes(d) == PEXPECT, r)
tampered = os.path.join(W, "p-tampered.purr")
t = bytearray(blob); t[len(t) // 3] ^= 4
open(tampered, "wb").write(t)
r = run("extract", tampered, fresh("p-tampered"), "Paw$ 12 ☕")
check("purr locked: a changed byte is 'damaged', not 'wrong password'", r.get("code") == "damaged", r)

safe = os.path.join(W, "p-safe.purr")
r = run("create", "purr", "normal", safe, "--repair", ptree)
plain_size = sizes["normal"]
check("purr repair data: adds about 3%", r.get("ok") and plain_size < os.path.getsize(safe) < plain_size * 1.06 + 8192, (plain_size, os.path.getsize(safe)))
check("purr repair data: verify says clean", run("verify", safe).get("report", {}).get("clean") is True)
whole = open(safe, "rb").read()
hurt = bytearray(whole)
hurt[2] ^= 0xFF                                    # the very first bytes
hurt[100000:100000 + plain_size // 50] = bytes(plain_size // 50)   # a hole of 2% in the data
hurt[plain_size - 300] ^= 1                        # the table of contents
broken = os.path.join(W, "p-broken.purr")
open(broken, "wb").write(hurt)
r = run("extract", broken, fresh("p-broken"))
check("purr damaged: unpacking fails and mentions repair data", r.get("code") == "damaged" and "repair data" in r.get("message", ""), r)
fixed = os.path.join(W, "p-fixed.purr")
r = run("repair", broken, fixed)
check("purr repair: succeeds", r.get("ok") and r["report"]["repaired"], r)
check("purr repair: the repaired copy is byte for byte the original archive", os.path.exists(fixed) and open(fixed, "rb").read() == whole)
check("purr repair: the damaged file was not touched", open(broken, "rb").read() == bytes(hurt))
d = fresh("p-fixed")
r = run("extract", fixed, d)
check("purr repair: repaired copy unpacks byte-exact", r.get("ok") and tree_hashes(d) == PEXPECT, r)
ruined = bytearray(whole); ruined[50000:50000 + plain_size // 8] = bytes(plain_size // 8)
open(broken, "wb").write(ruined)
r = run("repair", broken, os.path.join(W, "p-fixed2.purr"))
check("purr repair: too much damage fails cleanly, writes nothing", r.get("code") == "damaged" and not os.path.exists(os.path.join(W, "p-fixed2.purr")), r)
r = run("repair", os.path.join(W, "p-normal.purr"), os.path.join(W, "p-fixed3.purr"))
check("purr repair: an archive without repair data says so", r.get("ok") is False and "no repair data" in r.get("message", ""), r)
cut = os.path.join(W, "p-cut.purr")
open(cut, "wb").write(open(os.path.join(W, "p-normal.purr"), "rb").read()[: sizes["normal"] // 2])
r = run("list", cut)
check("purr cut short: clear error, no crash", r.get("code") == "damaged", r)
r = run("test", os.path.join(W, "p-max.purr"))
check("purr: test command", r.get("ok") and r["files"] == len([k for k in PEXPECT if not k.endswith("/")]), r)

# ---------- split archives (pieces of a chosen size: name.zip.001, .002, ...) ----------
for fmt in ["zip", "7z", "purr"]:
    base = os.path.join(W, f"split.{fmt}")
    extra = ["--repair"] if fmt == "purr" else []
    r = run("create", fmt, "store", base, "--split", "1MB", *extra, tree)
    pieces = sorted(f for f in os.listdir(W) if f.startswith(f"split.{fmt}."))
    check(f"split {fmt}: makes several pieces, none over 1 MB, no unsplit file left",
          r.get("ok") and len(pieces) >= 3 and all(os.path.getsize(os.path.join(W, f)) <= 1_000_000 for f in pieces)
          and not os.path.exists(base) and not os.path.exists(base + ".part") and len(r["report"]["pieces"]) == len(pieces), (r, pieces))
    check(f"split {fmt}: pieces are numbered 001 upwards with no gaps", pieces == [f"split.{fmt}.{i:03d}" for i in range(1, len(pieces) + 1)], pieces)
    for pick in [pieces[0], pieces[-1]]:
        d = fresh(f"split-{fmt}-{pick[-3:]}")
        r = run("extract", os.path.join(W, pick), d)
        check(f"split {fmt}: opening piece {pick[-3:]} unpacks the whole set byte-exact", r.get("ok") and tree_hashes(d) == EXPECT, r)
    joined = os.path.join(W, f"joined.{fmt}")
    with open(joined, "wb") as j:
        for f in pieces:
            j.write(open(os.path.join(W, f), "rb").read())
    d = fresh(f"split-{fmt}-joined")
    check(f"split {fmt}: pieces joined end to end are one normal archive (7-Zip style)", run("extract", joined, d).get("ok") and tree_hashes(d) == EXPECT)
    l = run("list", os.path.join(W, pieces[1]))
    check(f"split {fmt}: listing reports the pieces", l.get("split", {}).get("pieces") == len(pieces) and l["split"]["first"] == pieces[0], l.get("split"))
    if fmt == "purr":
        check("split purr: repair data check works across pieces", run("verify", os.path.join(W, pieces[0])).get("report", {}).get("clean") is True)
    last = os.path.join(W, pieces[-1]); os.rename(last, last + ".away")
    r = run("list", os.path.join(W, pieces[0]))
    check(f"split {fmt}: missing last piece explained in plain words", r.get("code") == "pieces_missing" and "missing" in r.get("message", ""), r)
    os.rename(last + ".away", last)
    mid = os.path.join(W, pieces[1]); os.rename(mid, mid + ".away")
    r = run("list", os.path.join(W, pieces[0]))
    check(f"split {fmt}: missing middle piece named", r.get("code") == "pieces_missing" and "Piece 2" in r.get("message", ""), r)
    os.rename(mid + ".away", mid)
# 7-Zip/HJSplit-style pieces made by a plain cut open too
whole = open(os.path.join(W, "joined.7z"), "rb").read()
for i in range(0, len(whole), 700_000):
    open(os.path.join(W, f"cut7.7z.{i // 700_000 + 1:03d}"), "wb").write(whole[i:i + 700_000])
d = fresh("split-cut7")
check("split: plain-cut .7z.001 pieces (as 7-Zip makes) open byte-exact", run("extract", os.path.join(W, "cut7.7z.001"), d).get("ok") and tree_hashes(d) == EXPECT)
r = run("create", "zip", "store", os.path.join(W, "fits.zip"), "--split", "500MB", tree)
check("split: an archive that fits in one piece stays one ordinary file", r.get("ok") and os.path.exists(os.path.join(W, "fits.zip")) and "pieces" not in r["report"], r)
r = run("create", "zip", "store", os.path.join(W, "split.zip"), "--split", "1MB", tree)
check("split: refuses when pieces with that name are already there", r.get("ok") is False and "already" in r.get("message", ""), r)
r = run("create", "zip", "store", os.path.join(W, "tiny.zip"), "--split", "1000", tree)
check("split: pieces under 1 MB refused", r.get("ok") is False and "1 MB" in r.get("message", ""), r)

print(f"\n{passed} passed, {len(failed)} failed")
if failed:
    print("FAILED:", *failed, sep="\n  ")
shutil.rmtree(W)
sys.exit(1 if failed else 0)
