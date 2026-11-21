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

print(f"\n{passed} passed, {len(failed)} failed")
if failed:
    print("FAILED:", *failed, sep="\n  ")
shutil.rmtree(W)
sys.exit(1 if failed else 0)
