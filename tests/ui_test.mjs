// Purrzip app-screen self-test: drives the real screen against the real engine (dev server)
// at phone and desktop sizes, Day and Night. Fails on page errors or any browser pop-up box.
// Usage: node ui_test.mjs <phone-root> <cli>   (dev server must run with PZ_ROOT=<phone-root>)
import { createRequire } from "module";
import { execFileSync } from "child_process";
import fs from "fs";
import crypto from "crypto";
import path from "path";
const require = createRequire(import.meta.url);
const puppeteer = require("puppeteer");
const BASE = process.env.BASE || "http://127.0.0.1:4231/";
const ROOT = process.argv[2], CLI = process.argv[3];
const DL = path.join(ROOT, "Download");
let pass = 0; const fails = [];
const ok = (name, cond, info) => { if (cond) { pass++; console.log("  ok  ", name); } else { fails.push(name); console.log("  FAIL", name, info ?? ""); } };
const wait = ms => new Promise(r => setTimeout(r, ms));

const b = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
async function fresh(width, height, theme) {
  const p = await b.newPage();
  p.errs = [];
  p.on("pageerror", e => p.errs.push(String(e)));
  p.on("dialog", d => { p.errs.push("NATIVE DIALOG " + d.type()); d.dismiss(); });
  await p.emulateMediaFeatures([{ name: "prefers-color-scheme", value: theme }]);
  await p.setViewport({ width, height });
  await p.goto(BASE);
  await p.waitForSelector("#home-open");
  // Bring a control to the middle of the screen before clicking it (the button bar sticks to the bottom).
  const rawClick = p.click.bind(p);
  p.click = async (sel, opts) => { await p.$eval(sel, e => e.scrollIntoView({ block: "center" })); return rawClick(sel, opts); };
  return p;
}
// Rows appear a moment after a folder is opened: poll for them instead of guessing a delay.
async function findRow(p, sel, text, act) {
  const t = Date.now();
  while (Date.now() - t < 8000) {
    const done = await p.evaluate((sel, text, act) => {
      const r = [...document.querySelectorAll(sel + " .row")].find(r => r.querySelector(".nm")?.textContent.trim().startsWith(text));
      if (!r) return false;
      (act === "check" ? r.querySelector("input") : r).click();
      return true;
    }, sel, text, act);
    if (done) return true;
    await wait(100);
  }
  return false;
}
const clickRow = (p, sel, text) => findRow(p, sel, text, "click");
const checkRow = (p, sel, text) => findRow(p, sel, text, "check");
const visible = (p, id) => p.$eval(id, e => !e.hidden && !e.closest("[hidden]"));
const text = (p, sel) => p.$eval(sel, e => e.textContent);
async function waitView(p, view, ms = 15000) {
  const t = Date.now();
  while (Date.now() - t < ms) { if (await visible(p, "#v-" + view)) return true; await wait(100); }
  return false;
}
async function openFromPicker(p, name) {
  await p.click("#home-open"); await wait(400);
  return clickRow(p, "#pick-list", name);
}
const cli = (...a) => { let out; try { out = execFileSync(CLI, a).toString(); } catch (e) { out = e.stdout.toString(); } return JSON.parse(out.trim().split("\n").pop()); };

for (const [label, w, h, theme] of [["phone/day", 390, 844, "light"], ["desktop/night", 1200, 860, "dark"]]) {
  console.log(`\n== ${label}`);
  const p = await fresh(w, h, theme);
  ok(`${label}: theme follows device`, (await p.$eval("html", e => e.dataset.theme)) === (theme === "dark" ? "dark" : "light"));

  // 1. open + browse inside + search
  ok(`${label}: picker lists archives`, await openFromPicker(p, "Cat photos.zip"));
  ok(`${label}: archive opens`, await waitView(p, "open"));
  ok(`${label}: summary shows 5 files`, (await text(p, "#o-summary")).startsWith("5 files"));
  await clickRow(p, "#o-list", "Cat photos"); await wait(150);
  await clickRow(p, "#o-list", "2026"); await wait(150);
  ok(`${label}: folder navigation`, (await text(p, "#o-crumbs")).includes("2026"));
  await p.type("#o-search", "notes"); await wait(150);
  ok(`${label}: search finds notes.txt`, (await p.$$eval("#o-list .row", r => r.length)) === 1);
  await clickRow(p, "#o-list", "Cat photos/notes.txt"); await wait(800);
  ok(`${label}: text preview`, (await p.$eval("#pv-body", e => e.textContent)).includes("Shopping list"));
  await p.keyboard.press("Escape"); await wait(100);
  ok(`${label}: Esc closes preview`, !(await visible(p, "#pv")));
  await p.$eval("#o-search", e => { e.value = ""; e.dispatchEvent(new Event("input")); });

  // 2. image preview
  await clickRow(p, "#o-list", "Cat photos"); await wait(100); await clickRow(p, "#o-list", "2026"); await wait(100);
  await clickRow(p, "#o-list", "party.png"); await wait(900);
  ok(`${label}: image preview loads`, await p.$eval("#pv-body img", i => i.complete && i.naturalWidth > 0).catch(() => false));
  await p.click("#pv-close");

  // 3. unzip only picked files
  await checkRow(p, "#o-list", "hello.png"); await checkRow(p, "#o-list", "peek.png");
  ok(`${label}: picked count on button`, (await text(p, "#o-some")).includes("2 picked"));
  await p.click("#o-some");
  ok(`${label}: picked unzip finishes`, await waitView(p, "done"));
  const where = await text(p, "#d-path");
  const folder = path.join(DL, where.split(" › ").slice(2).join("/"));
  const got = fs.existsSync(folder) ? execFileSync("find", [folder, "-type", "f"]).toString().trim().split("\n").map(f => path.basename(f)).sort() : [];
  ok(`${label}: only the 2 picked files written`, JSON.stringify(got) === '["hello.png","peek.png"]', got);
  fs.rmSync(folder, { recursive: true, force: true });
  await p.click("#d-again");

  // 4. locked 7z: cancel, wrong, right
  await p.click("#go-home");
  await openFromPicker(p, "Secret plans.7z"); await wait(500);
  ok(`${label}: password asked`, await visible(p, "#dlg"));
  await p.type("#dlg-pw", "wrong"); await p.click("#dlg-ok"); await wait(600);
  ok(`${label}: wrong password message`, (await text(p, "#dlg-err")).includes("not it"));
  await p.type("#dlg-pw", "meow"); await p.keyboard.press("Enter");
  ok(`${label}: right password opens (hidden names now visible)`, await waitView(p, "open") && (await p.$$eval("#o-list .row", r => r.length)) === 1);
  await p.click("#o-all");
  ok(`${label}: locked 7z unzips`, await waitView(p, "done"));
  const docs = path.join(DL, (await text(p, "#d-path")).split(" › ").slice(2).join("/"));
  ok(`${label}: 7z content right`, fs.readFileSync(path.join(docs, "list-3.txt"), "utf8").startsWith("Shopping list 3"));
  fs.rmSync(docs, { recursive: true, force: true });

  // 5. make a locked .zip from a folder
  await p.click("#go-home"); await p.click("#home-make"); await wait(200);
  ok(`${label}: Make button off with nothing picked`, await p.$eval("#m-go", b => b.disabled));
  await p.click("#m-add-files"); await wait(400);
  await p.click("#pick-up"); await wait(300);
  await checkRow(p, "#pick-list", "Pictures"); await p.click("#pick-ok"); await wait(400);
  ok(`${label}: picked folder listed`, (await p.$$eval("#m-items .row", r => r.length)) === 1);
  ok(`${label}: name suggested from folder`, (await p.$eval("#m-name", e => e.value)) === "Pictures");
  await p.click("#m-pw-on"); await p.type("#m-pw", "Purr 9"); await p.type("#m-pw2", "Purr 8");
  ok(`${label}: mismatched passwords block Make`, await p.$eval("#m-go", b => b.disabled) && (await text(p, "#m-pw-hint")).includes("don't match"));
  await p.$eval("#m-pw2", e => { e.value = ""; }); await p.type("#m-pw2", "Purr 9");
  ok(`${label}: zip lock warning mentions Windows`, (await text(p, "#m-pw-hint")).includes("Windows"));
  await p.click("#m-go");
  ok(`${label}: zip made`, await waitView(p, "done"));
  const zipPath = path.join(ROOT, (await text(p, "#d-path")).split(" › ").slice(1).join("/"));
  ok(`${label}: done screen says packed`, (await text(p, "#d-title")) === "Packed!");
  const zl = cli("list", zipPath, "Purr 9");
  ok(`${label}: made zip is locked (AES) and lists 7 pictures`, zl.any_encrypted && zl.entries.filter(e => !e.is_dir).length === 7, zl.entries.length);
  const out1 = fs.mkdtempSync(path.join(ROOT, "..", "chk-"));
  const r1 = cli("extract", zipPath, out1, "Purr 9");
  const orig = fs.readFileSync(path.join(ROOT, "Pictures/GmCat/done.png"));
  ok(`${label}: made zip opens byte-exact`, r1.ok && Buffer.compare(fs.readFileSync(path.join(out1, "Pictures/GmCat/done.png")), orig) === 0);
  ok(`${label}: wrong password refused`, cli("extract", zipPath, out1, "nope").code === "wrong_password");
  fs.rmSync(out1, { recursive: true, force: true }); fs.rmSync(zipPath);
  await p.click("#d-again");

  // 6. make a .7z with hidden names
  await p.click("#home-make"); await wait(200);
  ok(`${label}: make form reset after success`, (await p.$$eval("#m-items .row", r => r.length)) === 0 && !(await p.$eval("#m-pw-on", c => c.checked)));
  await p.click("#m-add-files"); await wait(400);
  await p.click("#pick-up"); await wait(300);
  await clickRow(p, "#pick-list", "Documents"); await wait(300);
  await checkRow(p, "#pick-list", "list-1.txt"); await checkRow(p, "#pick-list", "list-2.txt");
  ok(`${label}: picker counts 2`, (await text(p, "#pick-ok")).includes("2 items"));
  await p.click("#pick-ok"); await wait(400);
  await p.click('#m-format button[data-v="SevenZ"]'); await p.click('#m-level button[data-v="Max"]');
  await p.click("#m-pw-on"); await p.type("#m-pw", "shh"); await p.type("#m-pw2", "shh");
  ok(`${label}: hide-names option shown for 7z`, await visible(p, "#m-hide-row"));
  await p.$eval("#m-name", e => { e.value = ""; }); await p.type("#m-name", "My lists");
  await p.click("#m-go");
  ok(`${label}: 7z made`, await waitView(p, "done"));
  const sevenPath = path.join(ROOT, (await text(p, "#d-path")).split(" › ").slice(1).join("/"));
  ok(`${label}: 7z named as typed`, path.basename(sevenPath) === "My lists.7z", sevenPath);
  ok(`${label}: 7z names hidden without password`, cli("list", sevenPath).code === "need_password");
  ok(`${label}: 7z opens with password`, cli("list", sevenPath, "shh").entries.length === 2);
  fs.rmSync(sevenPath);

  // 7. tar.gz + safety notes on a booby-trapped zip
  await p.click("#d-again"); await p.click("#go-home");
  await openFromPicker(p, "evil.zip"); await waitView(p, "open");
  await p.click("#o-all");
  ok(`${label}: evil zip finishes`, await waitView(p, "done"));
  const notes = await text(p, "#d-notes");
  ok(`${label}: safety notes shown`, notes.includes("skipped to keep your device safe") && notes.includes("changed so they fit"), notes);
  const evilDir = path.join(DL, (await text(p, "#d-path")).split(" › ").slice(2).join("/"));
  ok(`${label}: nothing escaped the Download folder`, !fs.existsSync(path.join(ROOT, "evil.txt")) && !fs.existsSync(path.join(DL, "evil.txt")));
  fs.rmSync(evilDir, { recursive: true, force: true });

  // 8. stop a big job
  await p.click("#d-again"); await p.click("#go-home");
  await openFromPicker(p, "big.zip"); await waitView(p, "open");
  // Press Stop the moment unpacking starts: a warm disk unpacks the 300 MB test file in under half a second.
  await p.click("#o-all"); await waitView(p, "work");
  await p.$eval("#w-stop", b => b.click());
  ok(`${label}: stopping returns to the archive`, await waitView(p, "open"));
  const left = fs.readdirSync(DL).filter(n => n.startsWith("big") && n !== "big.zip" || n.startsWith(".purrzip"));
  ok(`${label}: nothing half-done left behind`, left.length === 0, left);

  // 8b. .purr: repeats stored once, photos shrunk, repair data, lock, and the repair flow
  await p.click("#go-home"); await p.click("#home-make"); await wait(200);
  await p.click("#m-add-files"); await wait(400);
  await p.click("#pick-up"); await wait(300);
  await checkRow(p, "#pick-list", "Pictures"); await p.click("#pick-ok"); await wait(400);
  ok(`${label}: .purr hint hidden for zip`, !(await visible(p, "#m-kind-hint")) && !(await visible(p, "#m-repair-row")));
  await p.click('#m-format button[data-v="Purr"]');
  ok(`${label}: .purr explains itself and offers repair data`, (await visible(p, "#m-kind-hint")) && (await visible(p, "#m-repair-row")) && !(await visible(p, "#m-hide-row")));
  await p.click("#m-repair");
  await p.$eval("#m-name", e => { e.value = ""; }); await p.type("#m-name", "Backup");
  await p.click("#m-go");
  ok(`${label}: purr made`, await waitView(p, "done", 60000) && (await text(p, "#d-title")) === "Packed!", await p.evaluate(() => ({ view: [...document.querySelectorAll(".view")].find(v => !v.hidden).id, go: document.getElementById("m-go").disabled, items: document.querySelectorAll("#m-items .row").length, pick: !document.getElementById("pick").hidden, dlg: !document.getElementById("dlg").hidden, pw: document.getElementById("m-pw-on").checked, toast: document.getElementById("toast").textContent, wtitle: document.getElementById("w-title").textContent, pct: document.getElementById("w-pct").textContent })));
  const purrPath = path.join(ROOT, (await text(p, "#d-path")).split(" › ").slice(1).join("/"));
  ok(`${label}: named Backup.purr`, path.basename(purrPath) === "Backup.purr", purrPath);
  const did = await text(p, "#d-notes");
  ok(`${label}: done screen says what .purr did`, did.includes("repeats") && did.includes("photo"), did);
  const pl = cli("list", purrPath);
  ok(`${label}: purr lists, has repair data, found the twin photo`, pl.format === "Purr" && pl.purr.repair_data && pl.purr.stats.dup_bytes > 50000 && pl.purr.stats.jpeg_count === 2, pl.purr);
  const out2 = fs.mkdtempSync(path.join(ROOT, "..", "chk-"));
  const r2 = cli("extract", purrPath, out2);
  ok(`${label}: purr unpacks byte-exact (photo)`, r2.ok && Buffer.compare(fs.readFileSync(path.join(out2, "Pictures/Trip/beach.jpg")), fs.readFileSync(path.join(ROOT, "Pictures/Trip/beach.jpg"))) === 0);
  fs.rmSync(out2, { recursive: true, force: true });
  await p.click("#d-again");
  // damage it, then open it in the app: the app offers to repair, repairs, and the copy opens
  const whole = fs.readFileSync(purrPath);
  const hurt = Buffer.from(whole); hurt[5] ^= 0xff; hurt.fill(0, 3000, 3900); hurt[whole.length - 40000] ^= 1;
  fs.writeFileSync(purrPath, hurt);
  await openFromPicker(p, "Backup.purr");
  ok(`${label}: damaged purr -> error with a repair offer`, await waitView(p, "error") && (await visible(p, "#e-repair")), await text(p, "#e-text"));
  await p.click("#e-repair");
  ok(`${label}: repair finishes`, await waitView(p, "done", 60000) && (await text(p, "#d-title")) === "Repaired!", await text(p, "#d-title"));
  const fixedPath = path.join(ROOT, (await text(p, "#d-path")).split(" › ").slice(1).join("/"));
  ok(`${label}: repaired copy is byte-identical to the undamaged archive`, path.basename(fixedPath) === "Backup (repaired).purr" && Buffer.compare(fs.readFileSync(fixedPath), whole) === 0, fixedPath);
  ok(`${label}: the damaged file was left alone`, Buffer.compare(fs.readFileSync(purrPath), hurt) === 0);
  await p.click("#d-show");
  ok(`${label}: "Open it" opens the repaired archive`, await waitView(p, "open") && (await text(p, "#o-name")) === "Backup (repaired).purr" && (await text(p, "#o-summary")).includes("repair data"));
  await p.click("#o-all");
  ok(`${label}: repaired purr unzips`, await waitView(p, "done"));
  const un = path.join(DL, (await text(p, "#d-path")).split(" › ").slice(2).join("/"));
  ok(`${label}: and the photo inside is right`, Buffer.compare(fs.readFileSync(path.join(un, "Trip/sunset.jpg")), fs.readFileSync(path.join(ROOT, "Pictures/Trip/sunset.jpg"))) === 0, un);
  fs.rmSync(un, { recursive: true, force: true }); fs.rmSync(purrPath); fs.rmSync(fixedPath);
  await p.click("#d-again"); await p.click("#go-home");
  // locked .purr: names hidden
  await p.click("#home-make"); await wait(200);
  await p.click("#m-add-files"); await wait(400);
  await p.click("#pick-up"); await wait(300);
  await checkRow(p, "#pick-list", "Documents"); await p.click("#pick-ok"); await wait(400);
  await p.click('#m-format button[data-v="Purr"]');
  await p.click("#m-pw-on"); await p.type("#m-pw", "hush"); await p.type("#m-pw2", "hush");
  ok(`${label}: purr lock hint says names are hidden`, (await text(p, "#m-pw-hint")).includes("names are hidden"));
  await p.click("#m-go");
  ok(`${label}: locked purr made`, await waitView(p, "done", 60000));
  const lockedPath = path.join(ROOT, (await text(p, "#d-path")).split(" › ").slice(1).join("/"));
  ok(`${label}: locked purr hides names`, cli("list", lockedPath).code === "need_password" && !fs.readFileSync(lockedPath).includes("list-1"));
  await p.click("#d-again"); await p.click("#go-home");
  await openFromPicker(p, path.basename(lockedPath)); await wait(600);
  ok(`${label}: locked purr asks for the password`, await visible(p, "#dlg"));
  await p.type("#dlg-pw", "wrong"); await p.click("#dlg-ok"); await wait(900);
  ok(`${label}: wrong purr password message`, (await text(p, "#dlg-err")).includes("not it"));
  await p.type("#dlg-pw", "hush"); await p.keyboard.press("Enter");
  ok(`${label}: right password opens the purr`, await waitView(p, "open") && (await p.$$eval("#o-list .row", r => r.length)) === 1);
  fs.rmSync(lockedPath);

  // 8b. split into pieces of a chosen size, opened back as one
  const vids = path.join(ROOT, "Videos"), clip = path.join(vids, "clip.bin");
  fs.mkdirSync(vids, { recursive: true });
  if (!fs.existsSync(clip)) fs.writeFileSync(clip, crypto.randomBytes(3_500_000));
  await p.click("#go-home"); await p.click("#home-make"); await wait(200);
  await p.click("#m-add-files"); await wait(400);
  await p.click("#pick-up"); await wait(300);
  await checkRow(p, "#pick-list", "Videos"); await p.click("#pick-ok"); await wait(400);
  ok(`${label}: split options hidden until asked`, !(await visible(p, "#m-split-box")));
  await p.click('#m-format button[data-v="Zip"]');
  await p.click("#m-split-on");
  ok(`${label}: split shows sizes, 450 MB picked, hint names the pieces`, (await visible(p, "#m-split-box")) && (await p.$eval('#m-split button[data-v="450000000"]', b => b.getAttribute("aria-checked"))) === "true" && (await text(p, "#m-split-hint")).includes("Videos.zip.001"), await text(p, "#m-split-hint"));
  await p.click('#m-split button[data-v="custom"]');
  ok(`${label}: own size: field shown, Make waits for a size`, (await visible(p, "#m-split-custom-row")) && await p.$eval("#m-go", b => b.disabled));
  await p.type("#m-split-custom", "0.5 MB");
  ok(`${label}: pieces under 1 MB refused`, await p.$eval("#m-go", b => b.disabled) && (await text(p, "#m-split-hint")).includes("at least 1 MB"));
  await p.$eval("#m-split-custom", e => { e.value = ""; }); await p.type("#m-split-custom", "1 MB");
  ok(`${label}: 1 MB accepted`, !(await p.$eval("#m-go", b => b.disabled)) && (await text(p, "#m-split-hint")).includes("1 MB at most"));
  await p.click('#m-format button[data-v="SevenZ"]');
  ok(`${label}: hint follows the kind (7z)`, (await text(p, "#m-split-hint")).includes("Videos.7z.001") && (await text(p, "#m-split-hint")).includes("7-Zip"));
  await p.click('#m-format button[data-v="Zip"]');
  ok(`${label}: hint follows the kind back to zip`, (await text(p, "#m-split-hint")).includes("Videos.zip.001"));
  await p.click("#m-go");
  ok(`${label}: split zip made`, await waitView(p, "done", 30000));
  const firstPiece = path.join(ROOT, (await text(p, "#d-path")).split(" › ").slice(1).join("/"));
  const pieces = fs.readdirSync(path.dirname(firstPiece)).filter(f => f.startsWith("Videos.zip.")).sort();
  ok(`${label}: 4 pieces named .001 to .004`, JSON.stringify(pieces) === '["Videos.zip.001","Videos.zip.002","Videos.zip.003","Videos.zip.004"]' && firstPiece.endsWith("Videos.zip.001"), pieces);
  ok(`${label}: done screen lists the pieces`, (await text(p, "#d-notes")).includes("4 pieces") && (await text(p, "#d-notes")).includes("Videos.zip.004"));
  ok(`${label}: no piece over 1 MB`, pieces.every(f => fs.statSync(path.join(path.dirname(firstPiece), f)).size <= 1_000_000));
  await p.click("#d-again"); await p.click("#go-home");
  await p.evaluate(f => window.pzOpenPaths([f]), path.join(path.dirname(firstPiece), "Videos.zip.003"));
  ok(`${label}: opening any piece opens the set`, await waitView(p, "open") && (await text(p, "#o-summary")).includes("4 pieces"));
  await p.click("#o-all");
  ok(`${label}: split zip unzips`, await waitView(p, "done", 30000));
  const unz = path.join(DL, (await text(p, "#d-path")).split(" › ").slice(2).join("/"));
  const back = fs.existsSync(path.join(unz, "clip.bin")) ? path.join(unz, "clip.bin") : unz;
  ok(`${label}: split zip comes back byte-exact`, fs.existsSync(back) && Buffer.compare(fs.readFileSync(back), fs.readFileSync(clip)) === 0, back);
  fs.rmSync(unz, { recursive: true, force: true });
  pieces.slice(0, 2).forEach(f => fs.renameSync(path.join(path.dirname(firstPiece), f), path.join(ROOT, "..", f)));
  await p.click("#d-again"); await p.click("#go-home");
  await p.evaluate(f => window.pzOpenPaths([f]), path.join(path.dirname(firstPiece), "Videos.zip.003"));
  ok(`${label}: missing first piece explained`, await waitView(p, "error") && (await text(p, "#e-text")).includes("first piece"), await text(p, "#e-text"));
  await p.click("#e-back");
  pieces.forEach(f => { fs.rmSync(path.join(path.dirname(firstPiece), f), { force: true }); fs.rmSync(path.join(ROOT, "..", f), { force: true }); });

  // 9. not an archive
  await p.click("#go-home");
  await openFromPicker(p, "not-an-archive.zip");
  ok(`${label}: friendly error for junk`, await waitView(p, "error") && (await text(p, "#e-text")).includes("doesn't look like an archive"));
  await p.click("#e-back");

  // 10. Day/Night toggle + About
  await p.click("#theme");
  ok(`${label}: theme toggles`, (await p.$eval("html", e => e.dataset.theme)) !== (theme === "dark" ? "dark" : "light"));
  await p.click("#about-btn");
  ok(`${label}: about shows version`, (await text(p, "#about-ver")).startsWith("v0."));
  await p.keyboard.press("Escape");
  ok(`${label}: Back function on home says 'leave'`, (await p.evaluate(() => window.pzBack())) === false);
  ok(`${label}: no horizontal scroll`, await p.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  ok(`${label}: no page errors or native pop-ups`, p.errs.length === 0, p.errs);
  await p.close();
}
await b.close();
console.log(`\n${pass} passed, ${fails.length} failed`);
if (fails.length) { console.log("FAILED:\n  " + fails.join("\n  ")); process.exit(1); }
