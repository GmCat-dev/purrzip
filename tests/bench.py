#!/usr/bin/env python3
"""Size and speed comparison on real folders: .zip, .7z and .purr from Purrzip, plus reference
tools when they are installed (xz, and real 7-Zip as `7zz` or `7z`).

Usage: python3 -I bench.py <purrzip-cli> <work dir> <name=folder> [<name=folder> ...]
Prints a table and writes <work dir>/bench.json. Every .purr is unpacked again and compared
with the folder, byte for byte.
"""
import filecmp, json, os, shutil, subprocess, sys, time

CLI, WORK = sys.argv[1], sys.argv[2]
CORPORA = [a.split("=", 1) for a in sys.argv[3:]]
os.makedirs(WORK, exist_ok=True)
SEVEN = shutil.which("7zz") or shutil.which("7z")
XZ = shutil.which("xz")


def folder_size(p):
    return sum(os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(p) for f in fs if not os.path.islink(os.path.join(d, f)))


def timed(cmd, **kw):
    t = time.time()
    r = subprocess.run(cmd, capture_output=True, **kw)
    return time.time() - t, r


def same_tree(a, b):
    """Byte-for-byte equal, ignoring links (Purrzip leaves links out on purpose and says so)."""
    c = filecmp.dircmp(a, b)
    left_only = [n for n in c.left_only if not os.path.islink(os.path.join(a, n))]
    if left_only or c.right_only or [n for n in c.funny_files if not os.path.islink(os.path.join(a, n))]:
        return False
    _, mismatch, errors = filecmp.cmpfiles(a, b, [n for n in c.common_files if not os.path.islink(os.path.join(a, n))], shallow=False)
    if mismatch or errors:
        return False
    return all(same_tree(os.path.join(a, d), os.path.join(b, d)) for d in c.common_dirs)


results = []
for name, folder in CORPORA:
    folder = os.path.abspath(folder)
    size = folder_size(folder)
    row = {"name": name, "size": size, "runs": {}}
    print(f"\n== {name}: {size / 1e6:.1f} MB", flush=True)

    def record(label, secs, out, note=""):
        n = os.path.getsize(out) if os.path.exists(out) else None
        row["runs"][label] = {"bytes": n, "secs": round(secs, 1), "note": note}
        pct = f"{n * 100 / size:6.2f}%" if n else "   fail"
        print(f"  {label:<24} {n or 0:>12,} B  {pct}  {secs:7.1f} s  {note}", flush=True)
        return n

    for label, args in [("zip normal", ["zip", "normal"]), ("7z normal (Purrzip)", ["7z", "normal"]), ("7z smallest (Purrzip)", ["7z", "max"]),
                        ("purr fast", ["purr", "fast"]), ("purr normal", ["purr", "normal"]), ("purr smallest", ["purr", "max"])]:
        ext = {"zip": "zip", "7z": "7z", "purr": "purr"}[args[0]]
        out = os.path.join(WORK, f"{name}-{args[1]}.{ext}")
        if os.path.exists(out):
            os.remove(out)
        secs, r = timed([CLI, "create", args[0], args[1], out, folder])
        note = ""
        if args[0] == "purr" and os.path.exists(out):
            rep = json.loads(r.stdout.decode().strip().splitlines()[-1]).get("report", {}).get("purr", {})
            dest = os.path.join(WORK, "unpack")
            shutil.rmtree(dest, ignore_errors=True)
            os.makedirs(dest)
            usecs, _ = timed([CLI, "extract", out, dest])
            ok = same_tree(folder, os.path.join(dest, os.path.basename(folder)))
            note = f"unpack {usecs:.1f} s, {'identical' if ok else 'MISMATCH'}; repeats {rep.get('dup_bytes', 0) / 1e6:.1f} MB, photos {rep.get('jpeg_in', 0) / 1e6:.1f}->{rep.get('jpeg_out', 0) / 1e6:.1f} MB"
            shutil.rmtree(dest, ignore_errors=True)
            row["runs"].setdefault("_checks", {})[label] = ok
        record(label, secs, out, note)
        if label != "purr smallest" and os.path.exists(out):
            os.remove(out)
    if XZ:
        out = os.path.join(WORK, f"{name}.tar.xz")
        t = time.time()
        with open(out, "wb") as o:
            tar = subprocess.Popen(["tar", "-C", os.path.dirname(folder), "--sort=name", "-cf", "-", os.path.basename(folder)], stdout=subprocess.PIPE)
            subprocess.run([XZ, "-9e", "-T1", "-c"], stdin=tar.stdout, stdout=o)
            tar.wait()
        record("tar + xz -9e", time.time() - t, out)
        os.remove(out)
    if SEVEN:
        for label, extra in [("7-Zip normal (real)", ["-mx=5"]), ("7-Zip ultra (real)", ["-mx=9"])]:
            out = os.path.join(WORK, f"{name}-real.7z")
            if os.path.exists(out):
                os.remove(out)
            secs, _ = timed([SEVEN, "a", "-bd", "-y", *extra, out, folder])
            record(label, secs, out)
            os.remove(out)
    results.append(row)

json.dump(results, open(os.path.join(WORK, "bench.json"), "w"), indent=1)
print("\nSummary (size as % of the folder; smaller is better)")
labels = [l for l in results[0]["runs"] if not l.startswith("_")] if results else []
print(f"{'':<16}" + "".join(f"{l[:13]:>14}" for l in labels))
for row in results:
    print(f"{row['name'][:15]:<16}" + "".join(f"{(row['runs'][l]['bytes'] or 0) * 100 / row['size']:>13.2f}%" for l in labels))
bad = [r["name"] for r in results for ok in r["runs"].get("_checks", {}).values() if not ok]
sys.exit(1 if bad else 0)
