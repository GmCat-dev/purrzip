// Right-click menu actions (Windows passes --extract-here etc.; here they are fed to the page directly through
// window.pzLaunch, against the real engine in the test server). usage: node tests/menu_test.mjs <folder with Trip.purr, single.zip, notes.txt, todo.txt, Trip/>
import { createRequire } from "module";
import fs from "fs";
import path from "path";
const require = createRequire(import.meta.url);
let puppeteer;
try { puppeteer = require("puppeteer"); } catch { puppeteer = require("puppeteer"); }

const D = path.resolve(process.argv[2]);
const BASE = process.env.PZ_URL || "http://127.0.0.1:4231/";
let fails = 0;
const ok = (name, cond) => { console.log((cond ? "  ok   " : "  FAIL ") + name); if (!cond) fails++; };
const sleep = ms => new Promise(r => setTimeout(r, ms));

const b = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
const p = await b.newPage();
const errs = [];
p.on("pageerror", e => errs.push(String(e)));
p.on("dialog", d => { errs.push("native dialog: " + d.message()); d.dismiss(); });
await p.setViewport({ width: 900, height: 800 });
await p.goto(BASE, { waitUntil: "networkidle0" });
const title = () => p.$eval("#d-title", e => e.textContent).catch(() => "");
async function waitDone(ms = 15000) {
  for (let t = 0; t < ms; t += 200) {
    const shown = await p.$eval("#v-done", e => !e.hidden).catch(() => false);
    if (shown) return title();
    await sleep(200);
  }
  return "";
}
async function launch(mode, paths) {
  // leave any result screen first, so the next wait sees the new result
  await p.evaluate(() => { const v = document.getElementById("v-done"); if (v && !v.hidden) document.getElementById("d-again").click(); });
  await sleep(300);
  await p.evaluate((m, ps) => window.pzLaunch({ mode: m, paths: ps }), mode, paths);
}

// Extract to a new folder: a single top folder still gets wrapped in "Trip (n)"-style own folder
await launch("extract-new", [path.join(D, "Trip.purr")]);
ok("extract-new finishes", /unpacked|Unzipped/i.test(await waitDone()));
const wrapped = fs.readdirSync(D).filter(n => /^Trip( \(\d+\))?$/.test(n) && n !== "Trip");
ok("extract-new makes its own folder (" + wrapped.join(",") + ")", wrapped.length === 1 && fs.existsSync(path.join(D, wrapped[0], "Trip", "a.jpg")));

// Extract here: one file inside -> lands next to the archive
await launch("extract-here", [path.join(D, "single.zip")]);
ok("extract-here finishes", /unpacked|Unzipped/i.test(await waitDone()));
ok("extract-here puts the file next to the archive", fs.readdirSync(D).some(n => /^notes \(\d+\)\.txt$/.test(n)));

// Test archive
await launch("test", [path.join(D, "Trip.purr")]);
ok("test says all good", (await waitDone()) === "All good!");
const txt = await p.$eval("#d-text", e => e.textContent);
ok("test counts the files (" + txt + ")", /1 file/.test(txt));

// A damaged archive: test reports a problem
const bad = path.join(D, "broken.purr");
const buf = fs.readFileSync(path.join(D, "Trip.purr")); buf[buf.length >> 1] ^= 0xff; fs.writeFileSync(bad, buf);
await launch("test", [bad]);
let shownErr = false;
for (let t = 0; t < 10000 && !shownErr; t += 200) { shownErr = await p.$eval("#v-error", e => !e.hidden).catch(() => false); await sleep(200); }
ok("test on a damaged archive shows a problem", shownErr);
fs.unlinkSync(bad);

// Add to a .purr archive: two files picked = two launches, gathered into one archive named after the folder
const before = new Set(fs.readdirSync(D));
await launch("add-purr", [path.join(D, "notes.txt")]);
await sleep(200);
await launch("add-purr", [path.join(D, "todo.txt")]);
ok("add-purr finishes", (await waitDone(20000)) === "Packed!");
const made = fs.readdirSync(D).filter(n => !before.has(n) && n.endsWith(".purr"));
ok("one archive named after the folder (" + made.join(",") + ")", made.length === 1 && made[0].startsWith(path.basename(D)));

// Add to a .zip archive: one folder -> "Trip.zip"
await launch("add-zip", [path.join(D, "Trip")]);
ok("add-zip finishes", (await waitDone(20000)) === "Packed!");
ok("Trip.zip made", fs.existsSync(path.join(D, "Trip.zip")));

// Add to archive...: opens the Make screen with the item
await launch("add", [path.join(D, "notes.txt")]);
await sleep(1500);
const makeShown = await p.$eval("#v-make", e => !e.hidden).catch(() => false);
ok("add opens the Make screen", makeShown);

ok("no page errors or native pop-ups (" + errs.join(" | ") + ")", errs.length === 0);
await b.close();
console.log(`\n${fails ? fails + " failed" : "all passed"}`);
process.exit(fails ? 1 : 0);
