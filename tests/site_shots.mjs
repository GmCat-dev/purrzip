// Phone screenshots of the real app (against the dev server) for purr.gmcat.io: home, looking inside, making a .purr.
// usage: node tests/site_shots.mjs <out folder>   (writes home/open/make-day/night.png, 540x1191)
import { createRequire } from "module";
const require = createRequire(import.meta.url);
let puppeteer;
try { puppeteer = require("puppeteer"); } catch { puppeteer = require("puppeteer"); }
const BASE = process.env.BASE || "http://127.0.0.1:4231/";
const OUT = process.argv[2];
const wait = ms => new Promise(r => setTimeout(r, ms));
const b = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
const errs = [];
for (const theme of ["light", "dark"]) {
  const name = theme === "light" ? "day" : "night";
  const p = await b.newPage();
  p.on("pageerror", e => errs.push(String(e)));
  await p.emulateMediaFeatures([{ name: "prefers-color-scheme", value: theme }]);
  await p.setViewport({ width: 360, height: 794, deviceScaleFactor: 1.5 });
  await p.goto(BASE, { waitUntil: "networkidle0" }); await wait(600);
  await p.screenshot({ path: `${OUT}/home-${name}.png` });
  await p.click("#home-open"); await wait(500);
  await p.evaluate(() => [...document.querySelectorAll("#pick-list .row")].find(r => r.textContent.includes("Cat photos.zip")).click()); await wait(800);
  await p.screenshot({ path: `${OUT}/open-${name}.png` });
  await p.click("#go-home"); await wait(200); await p.click("#home-make"); await wait(300);
  await p.click("#m-add-files"); await wait(500);
  await p.click("#pick-up"); await wait(400);
  await p.evaluate(() => [...document.querySelectorAll("#pick-list .row")].find(r => r.textContent.includes("Pictures")).click()); await wait(400);
  await p.evaluate(() => [...document.querySelectorAll("#pick-list .row input")].forEach(c => c.click()));
  await p.click("#pick-ok"); await wait(500);
  await p.click('#m-format button[data-v="Purr"]'); await wait(300);
  await p.evaluate(() => window.scrollTo(0, 0)); await wait(300);
  await p.screenshot({ path: `${OUT}/make-${name}.png` });
  await p.close();
}
console.log(JSON.stringify({ errs }));
await b.close();
