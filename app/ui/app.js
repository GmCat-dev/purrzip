// Purrzip app screen. Talks to the engine through PZ (bridge.js).
(function () {
  "use strict";
  var $ = function (id) { return document.getElementById(id); };
  var info = { platform: "", in_app_browser: true };
  var S = {
    open: null,       // { path, password, listing, cwd, selected:Set, search, dest, tree }
    make: { items: [], format: "Zip", level: "Normal", dest: null },
    job: null,        // { id, kind, back }
    lastResult: null,
  };

  // ---------- helpers ----------
  function size(n) {
    if (n == null) return "";
    if (n < 1024) return n + " B";
    var u = ["KB", "MB", "GB", "TB"], i = -1;
    do { n /= 1024; i++; } while (n >= 1024 && i < u.length - 1);
    return (n >= 100 ? Math.round(n) : n.toFixed(1)) + " " + u[i];
  }
  function plural(n, one, many) { return n + " " + (n === 1 ? one : (many || one + "s")); }
  function baseName(p) { return String(p).split(/[\\/]/).filter(Boolean).pop() || String(p); }
  function el(tag, cls, text) { var e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; }
  function show(view) {
    document.querySelectorAll(".view").forEach(function (v) { v.hidden = v.id !== "v-" + view; });
    window.scrollTo(0, 0);
  }
  var toastTimer;
  function toast(msg) {
    var t = $("toast"); t.textContent = msg; t.hidden = false;
    clearTimeout(toastTimer); toastTimer = setTimeout(function () { t.hidden = true; }, 3200);
  }
  var FRIENDLY = {
    unknown: "This doesn't look like an archive Purrzip can open.",
    damaged: "This archive looks damaged or incomplete. Maybe the download stopped early?",
    disk_full: "The disk is full. Free up some space and try again.",
    need_password: "This archive needs a password.",
    wrong_password: "That password doesn't open this archive.",
  };
  function friendly(err) { return FRIENDLY[err && err.code] || (err && err.message) || "Something went wrong."; }
  // `path` is the archive that failed; a damaged .purr with repair data gets a "Try to repair" button.
  function fail(title, err, path) {
    $("e-title").textContent = title;
    S.repairPath = err && err.repairable && path ? path : null;
    $("e-text").textContent = S.repairPath
      ? "This archive is damaged, but it carries repair data, so Purrzip can try to fix it. Your file stays as it is; the fix goes into a copy."
      : friendly(err);
    $("e-repair").hidden = !S.repairPath;
    $("e-back").classList.toggle("primary", !S.repairPath);
    show("error");
  }


  // Small drawn icons (emoji differ between phones and PCs).
  var ICON = {
    folder: '<path d="M2 6.5A1.5 1.5 0 0 1 3.5 5H8l2 2h6.5A1.5 1.5 0 0 1 18 8.5v7a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 2 15.5z" fill="#F2B04A" stroke="#B9782A" stroke-width="1"/>',
    file: '<path d="M5 2.5h7l4 4v11H5z" fill="var(--panel)" stroke="var(--muted)" stroke-width="1.3" stroke-linejoin="round"/><path d="M12 2.5v4h4" fill="none" stroke="var(--muted)" stroke-width="1.3"/><path d="M7.5 10h6M7.5 12.5h6M7.5 15h4" stroke="var(--muted)" stroke-width="1.1"/>',
    archive: '<rect x="3" y="3" width="14" height="14" rx="2.5" fill="#8C82B8" stroke="#5E5590" stroke-width="1"/><path d="M10 3v2.5M10 7v2" stroke="#fff" stroke-width="1.6" stroke-dasharray="1.4 1"/><rect x="8.2" y="9.5" width="3.6" height="4.2" rx="1" fill="#fff"/>',
    image: '<rect x="2.5" y="4" width="15" height="12" rx="2" fill="#7FB8E6" stroke="#3B73D1" stroke-width="1"/><circle cx="7" cy="8" r="1.6" fill="#fff"/><path d="M3 15l4.5-4.5 3 3 2.5-2.5 4.5 4.5" fill="#3E9B72"/>',
    video: '<rect x="2.5" y="4.5" width="15" height="11" rx="2" fill="#E07A5F" stroke="#A84F38" stroke-width="1"/><path d="M8.5 7.5v5l4.2-2.5z" fill="#fff"/>',
    music: '<circle cx="10" cy="10" r="7.5" fill="#B9A7E8" stroke="#7C6BB8" stroke-width="1"/><path d="M8.5 13V6.5l4-1V12" fill="none" stroke="#fff" stroke-width="1.4"/><circle cx="7.6" cy="13" r="1.4" fill="#fff"/><circle cx="11.6" cy="12" r="1.4" fill="#fff"/>',
    doc: '<path d="M5 2.5h7l4 4v11H5z" fill="#F28A3A" stroke="#B9602A" stroke-width="1" stroke-linejoin="round"/><path d="M7.5 10h6M7.5 12.5h6M7.5 15h4" stroke="#fff" stroke-width="1.2"/>',
    link: '<path d="M8.5 11.5l3-3M7 9l-1.8 1.8a2.5 2.5 0 0 0 3.5 3.5L10.5 12.5M13 11l1.8-1.8a2.5 2.5 0 0 0-3.5-3.5L9.5 7.5" fill="none" stroke="var(--muted)" stroke-width="1.6" stroke-linecap="round"/>',
  };
  function icon(kind) {
    var s = el("span", "ic");
    s.innerHTML = '<svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">' + ICON[kind] + "</svg>";
    return s;
  }
  function kindFor(n) {
    if (/\.(png|jpe?g|gif|webp|bmp|avif|heic|svg)$/i.test(n)) return "image";
    if (/\.(mp4|mov|mkv|webm|avi|m4v)$/i.test(n)) return "video";
    if (/\.(mp3|m4a|wav|flac|ogg|opus|aac)$/i.test(n)) return "music";
    if (/\.(pdf|docx?|odt|rtf|xlsx?|pptx?)$/i.test(n)) return "doc";
    if (looksArchive(n)) return "archive";
    return "file";
  }
  // "Phone storage › Download" instead of the raw path, when inside a known place.
  var placesCache = [];
  function nicePath(p) {
    for (var i = 0; i < placesCache.length; i++) {
      var pl = placesCache[i], base = String(pl.path).replace(/[\\/]+$/, "");
      if (p === base || p.indexOf(base + "/") === 0 || p.indexOf(base + "\\") === 0) {
        var rest = p.slice(base.length).split(/[\\/]/).filter(Boolean);
        return [pl.label].concat(rest).join(" › ");
      }
    }
    return p;
  }

  // ---------- in-page dialog ----------
  var dlgResolve = null;
  function ask(o) {
    $("dlg-title").textContent = o.title || "";
    $("dlg-text").textContent = o.text || "";
    $("dlg-cat").src = "img/" + (o.cat || "secret") + ".png";
    $("dlg-pw-row").hidden = !o.password;
    $("dlg-pw").value = "";
    $("dlg-pw").type = "password";
    $("dlg-err").hidden = !o.error; $("dlg-err").textContent = o.error || "";
    $("dlg-ok").textContent = o.ok || "OK";
    $("dlg-cancel").textContent = o.cancel || "Cancel";
    $("dlg-cancel").hidden = o.cancel === false;
    $("dlg").hidden = false;
    var box = $("dlg").querySelector(".dialog");
    if (o.error) { box.classList.remove("shake"); void box.offsetWidth; box.classList.add("shake"); }
    setTimeout(function () { (o.password ? $("dlg-pw") : (o.danger ? $("dlg-cancel") : $("dlg-ok"))).focus(); }, 30);
    return new Promise(function (res) { dlgResolve = function (v) { $("dlg").hidden = true; dlgResolve = null; res(v); }; });
  }
  $("dlg-ok").onclick = function () { if (dlgResolve) dlgResolve($("dlg-pw-row").hidden ? true : $("dlg-pw").value); };
  $("dlg-cancel").onclick = function () { if (dlgResolve) dlgResolve(null); };
  $("dlg").addEventListener("click", function (e) { if (e.target === $("dlg") && dlgResolve) dlgResolve(null); });
  $("dlg-pw").addEventListener("keydown", function (e) { if (e.key === "Enter") $("dlg-ok").click(); });
  document.querySelectorAll("[data-show]").forEach(function (b) {
    b.onclick = function () { var i = $(b.dataset.show); var hid = i.type === "password"; i.type = hid ? "text" : "password"; b.textContent = hid ? "Hide" : "Show"; };
  });
  function askPassword(name, error) {
    return ask({ title: "This one's locked", text: "Type the password for " + name + ".", cat: "secret", password: true, ok: "Open", error: error });
  }

  // ---------- in-app file browser ----------
  var P = null; // { mode: 'archive'|'items'|'folder'|'view', dir, chosen: Map, resolve }
  async function picker(mode, start, title) {
    if (window.pzNeedStorage && !(await window.pzNeedStorage())) return null;
    var places = [];
    try { places = await PZ.call("places"); placesCache = places; } catch (e) {}
    var box = $("pick-places"); box.textContent = "";
    places.forEach(function (pl) {
      var b = el("button", null, pl.label); b.type = "button";
      b.onclick = function () { browseTo(pl.path); };
      box.appendChild(b);
    });
    $("pick-title").textContent = title || "Pick";
    $("pick").hidden = false;
    return new Promise(function (res) {
      P = { mode: mode, dir: null, chosen: new Map(), resolve: res };
      browseTo(start || (places[0] && places[0].path) || null);
    });
  }
  async function browseTo(dir) {
    var r;
    try { r = await PZ.call("browse", dir ? { dir: dir } : {}); }
    catch (e) { toast(friendly(e)); if (!P.dir) closePicker(null); return; }
    P.dir = r.dir;
    $("pick-path").textContent = nicePath(r.dir);
    $("pick-up").disabled = !r.parent;
    $("pick-up").onclick = function () { if (r.parent) browseTo(r.parent); };
    var list = $("pick-list"); list.textContent = "";
    var items = r.items;
    if (!items.length) list.appendChild(el("p", "empty", "This folder is empty."));
    items.forEach(function (it) {
      var checkable = P.mode === "items";
      var row = el("div", "row" + (checkable ? "" : " nocheck")); row.setAttribute("role", "listitem");
      if (checkable) {
        var cb = el("input"); cb.type = "checkbox"; cb.checked = P.chosen.has(it.path);
        cb.setAttribute("aria-label", "Pick " + it.name);
        cb.onclick = function (e) { e.stopPropagation(); toggle(it, cb.checked, row); };
        row.appendChild(cb);
        if (cb.checked) row.classList.add("sel");
      }
      row.appendChild(icon(it.is_dir ? "folder" : kindFor(it.name)));
      row.appendChild(el("span", "nm", it.name));
      row.appendChild(el("span", "sz", it.is_dir ? "" : size(it.size)));
      if (P.mode === "folder" && !it.is_dir) row.classList.add("dim");
      row.onclick = function () {
        if (it.is_dir) return browseTo(it.path);
        if (P.mode === "archive") return closePicker([it.path]);
        if (P.mode === "items") { var c = row.querySelector("input"); c.checked = !c.checked; toggle(it, c.checked, row); }
      };
      list.appendChild(row);
    });
    updatePickFoot();
  }
  function toggle(it, on, row) {
    if (on) P.chosen.set(it.path, it); else P.chosen.delete(it.path);
    row.classList.toggle("sel", on);
    updatePickFoot();
  }
  function updatePickFoot() {
    var ok = $("pick-ok"), c = $("pick-count");
    ok.hidden = false; c.textContent = "";
    if (P.mode === "folder") ok.textContent = "Use this folder";
    else if (P.mode === "items") { ok.textContent = P.chosen.size ? "Add " + plural(P.chosen.size, "item") : "Add"; ok.disabled = !P.chosen.size; return; }
    else if (P.mode === "archive") { ok.hidden = true; c.textContent = "Tap an archive to open it."; }
    else if (P.mode === "view") ok.textContent = "Close";
    ok.disabled = false;
  }
  function looksArchive(n) { return /\.(zip|7z|purr|rar|tar|tgz|gz|bz2|xz|zst|iso|cab|tbz2|txz|jar|apk)$/i.test(n); }
  function closePicker(v) { $("pick").hidden = true; var p = P; P = null; if (p) p.resolve(v); }
  $("pick-close").onclick = function () { closePicker(null); };
  $("pick-ok").onclick = function () {
    if (!P) return;
    if (P.mode === "folder") closePicker([P.dir]);
    else if (P.mode === "items") closePicker(Array.from(P.chosen.values()).map(function (i) { return i.path; }));
    else closePicker(null);
  };

  async function chooseFolder(start, title) {
    var r = await PZ.pick({ directory: true, title: title });
    if (r === null) r = await picker("folder", start, title);
    return r && r.length ? r[0] : null;
  }

  // ---------- opening an archive ----------
  async function openArchive(path, password) {
    var listing;
    try { listing = await PZ.call("list", { path: path, password: password || "" }); }
    catch (e) {
      if (e.code === "need_password" || (e.code === "wrong_password" && password)) {
        var pw = await askPassword(baseName(path), e.code === "wrong_password" ? "That's not it. Try again?" : null);
        if (pw === null) return;
        return openArchive(path, pw);
      }
      S.open = null;
      return fail("Couldn't open " + baseName(path), e, path);
    }
    S.open = { path: path, password: password || "", listing: listing, cwd: "", selected: new Set(), search: "", dest: null };
    S.open.tree = buildTree(listing.entries);
    S.open.dest = defaultDest(listing.folder);
    $("o-name").textContent = listing.name;
    $("o-format").textContent = listing.label;
    var files = listing.entries.filter(function (e) { return !e.is_dir && !e.special; }).length;
    $("o-summary").textContent = plural(files, "file") + (listing.size_unknown ? "" : " · " + size(listing.total_size) + " unpacked") + " · " + size(listing.file_size) + " packed" + (listing.any_encrypted || password ? " · 🔒" : "") + (listing.purr && listing.purr.repair_data ? " · repair data ✓" : "");
    $("o-search").value = "";
    $("o-dest").textContent = S.open.dest ? nicePath(S.open.dest) : "(pick a folder)";
    renderOpen();
    show("open");
  }
  function defaultDest(archiveFolder) {
    if (info.platform !== "android" && archiveFolder) return archiveFolder;
    return info.downloads || archiveFolder || null;
  }
  // Folders may be listed or only implied by file paths; build a full tree either way.
  function buildTree(entries) {
    var dirs = new Map(); // path -> { name, path, index, dirs:Set, files:[] }
    function dir(p) {
      if (dirs.has(p)) return dirs.get(p);
      var d = { name: p.split("/").pop(), path: p, index: null, dirs: new Set(), files: [] };
      dirs.set(p, d);
      if (p !== "") { var parent = p.includes("/") ? p.slice(0, p.lastIndexOf("/")) : ""; dir(parent).dirs.add(p); }
      return d;
    }
    dir("");
    entries.forEach(function (e) {
      var p = e.path.replace(/^\/+/, "");
      if (e.is_dir) { dir(p).index = e.index; return; }
      var parent = p.includes("/") ? p.slice(0, p.lastIndexOf("/")) : "";
      dir(parent).files.push(e);
    });
    return dirs;
  }
  function descendants(d, out) {
    out = out || [];
    if (d.index != null) out.push(d.index);
    d.files.forEach(function (f) { out.push(f.index); });
    d.dirs.forEach(function (p) { descendants(S.open.tree.get(p), out); });
    return out;
  }
  function renderOpen() {
    var O = S.open, list = $("o-list"); list.textContent = "";
    var crumbs = $("o-crumbs"); crumbs.textContent = "";
    var q = O.search.trim().toLowerCase();
    var parts = O.cwd ? O.cwd.split("/") : [];
    var top = el("button", null, "All files"); top.type = "button"; top.onclick = function () { O.cwd = ""; renderOpen(); };
    crumbs.appendChild(top);
    parts.forEach(function (name, i) {
      crumbs.appendChild(el("span", null, "/"));
      var b = el("button", null, name); b.type = "button";
      var target = parts.slice(0, i + 1).join("/");
      b.onclick = function () { O.cwd = target; renderOpen(); };
      crumbs.appendChild(b);
    });
    var rows = [];
    if (q) {
      O.listing.entries.forEach(function (e) {
        if (!e.is_dir && e.path.toLowerCase().includes(q)) rows.push({ file: e, label: e.path });
      });
    } else {
      var d = O.tree.get(O.cwd) || O.tree.get("");
      Array.from(d.dirs).sort(function (a, b) { return a.localeCompare(b); }).forEach(function (p) { rows.push({ dir: O.tree.get(p) }); });
      d.files.slice().sort(function (a, b) { return a.path.localeCompare(b.path); }).forEach(function (f) { rows.push({ file: f, label: f.path.split("/").pop() }); });
    }
    if (!rows.length) list.appendChild(el("p", "empty", q ? "No files match." : "This folder is empty."));
    var LIMIT = 1500;
    rows.slice(0, LIMIT).forEach(function (r) {
      var row = el("div", "row"); row.setAttribute("role", "listitem");
      var cb = el("input"); cb.type = "checkbox";
      if (r.dir) {
        var all = descendants(r.dir), n = all.filter(function (i) { return O.selected.has(i); }).length;
        cb.checked = all.length > 0 && n === all.length; cb.indeterminate = n > 0 && n < all.length;
        cb.setAttribute("aria-label", "Pick folder " + r.dir.name);
        cb.onclick = function (e) { e.stopPropagation(); all.forEach(function (i) { cb.checked ? O.selected.add(i) : O.selected.delete(i); }); renderOpen(); };
        var count = all.length - (r.dir.index != null ? 1 : 0);
        row.append(cb, icon("folder"), el("span", "nm", r.dir.name), el("span", "sz", plural(count, "item")));
        row.onclick = function () { O.cwd = r.dir.path; renderOpen(); };
        if (cb.checked) row.classList.add("sel");
      } else {
        var f = r.file;
        cb.checked = O.selected.has(f.index);
        cb.setAttribute("aria-label", "Pick " + r.label);
        cb.onclick = function (e) { e.stopPropagation(); cb.checked ? O.selected.add(f.index) : O.selected.delete(f.index); row.classList.toggle("sel", cb.checked); updateOpenButtons(); };
        var nm = el("span", "nm", r.label);
        if (f.encrypted) nm.appendChild(el("small", null, "🔒"));
        if (f.special) { nm.appendChild(el("small", null, "link, won't be unzipped")); row.classList.add("dim"); }
        row.append(cb, icon(f.special ? "link" : kindFor(f.path)), nm, el("span", "sz", O.listing.size_unknown ? "" : size(f.size)));
        row.onclick = function () { if (!f.special) preview(f); };
        if (cb.checked) row.classList.add("sel");
      }
      list.appendChild(row);
    });
    if (rows.length > LIMIT) list.appendChild(el("p", "empty", "…and " + (rows.length - LIMIT) + " more. Use Find to narrow it down."));
    updateOpenButtons();
  }
  function updateOpenButtons() {
    var n = S.open.selected.size;
    $("o-some").hidden = n === 0;
    $("o-some").textContent = "Unzip " + plural(n, "picked item");
  }
  $("o-search").addEventListener("input", function () { S.open.search = this.value; renderOpen(); });
  $("o-back").onclick = function () { S.open = null; show("home"); };
  $("o-dest-change").onclick = async function () {
    var d = await chooseFolder(S.open.dest, "Unzip into which folder?");
    if (d) { S.open.dest = d; $("o-dest").textContent = nicePath(d); }
  };
  $("o-all").onclick = function () { startExtract(null); };
  $("o-some").onclick = function () { startExtract(Array.from(S.open.selected)); };

  async function ensurePassword() {
    var O = S.open;
    if (O.listing.any_encrypted && !O.password) {
      var pw = await askPassword(O.listing.name);
      if (pw === null) return false;
      O.password = pw;
    }
    return true;
  }
  async function startExtract(only, retryError) {
    var O = S.open;
    if (!O.dest) { var d = await chooseFolder(null, "Unzip into which folder?"); if (!d) return; O.dest = d; $("o-dest").textContent = nicePath(d); }
    if (!(await ensurePassword())) return;
    var r;
    try { r = await PZ.call("extract", { path: O.path, dest: O.dest, password: O.password, only: only, own_folder: !!O.ownFolder }); }
    catch (e) { return fail("Couldn't start", e); }
    runJob(r.id, "extract", { only: only });
  }

  // ---------- preview ----------
  var pvEntry = null;
  async function preview(f) {
    var O = S.open;
    if (f.encrypted && !O.password) { if (!(await ensurePassword())) return; }
    pvEntry = f;
    var body = $("pv-body"); body.textContent = "";
    body.appendChild(el("p", "empty", "Opening…"));
    $("pv-title").textContent = f.path.split("/").pop();
    $("pv").hidden = false;
    var r;
    try { r = await PZ.call("preview", { path: O.path, password: O.password, index: f.index }); }
    catch (e) {
      if (e.code === "wrong_password" || e.code === "need_password") {
        $("pv").hidden = true;
        var pw = await askPassword(O.listing.name, e.code === "wrong_password" ? "That's not it. Try again?" : null);
        if (pw === null) return;
        O.password = pw; return preview(f);
      }
      body.textContent = ""; body.appendChild(el("p", "empty", friendly(e))); return;
    }
    body.textContent = "";
    var url = PZ.fileUrl(r.file);
    if (r.kind === "image") { var img = el("img"); img.alt = r.name; img.src = url; body.appendChild(img); }
    else if (r.kind === "video") { var v = el("video"); v.controls = true; v.src = url; v.playsInline = true; body.appendChild(v); }
    else if (r.kind === "audio") { var a = el("audio"); a.controls = true; a.src = url; body.appendChild(a); }
    else if (r.kind === "pdf" && info.platform !== "android") { var fr = el("iframe"); fr.src = url; fr.title = r.name; body.appendChild(fr); }
    else if (r.kind === "text") { var pre = el("pre", null, r.text + (r.cut ? "\n\n… (only the start is shown)" : "")); body.appendChild(pre); }
    else body.appendChild(el("p", "empty", "Purrzip can't show this kind of file here (" + size(r.size) + "). Unzip it to open it with another app."));
  }
  function closePreview() { $("pv").hidden = true; $("pv-body").textContent = ""; }
  $("pv-close").onclick = closePreview; $("pv-close2").onclick = closePreview;
  $("pv-extract").onclick = function () { closePreview(); if (pvEntry) startExtract([pvEntry.index]); };

  // ---------- making an archive ----------
  function renderMake() {
    var M = S.make, list = $("m-items");
    list.querySelectorAll(".row").forEach(function (r) { r.remove(); });
    $("m-empty").hidden = M.items.length > 0;
    M.items.forEach(function (it, i) {
      var row = el("div", "row nocheck"); row.setAttribute("role", "listitem");
      var x = el("button", "x", "✕"); x.type = "button"; x.setAttribute("aria-label", "Remove " + it.name);
      x.onclick = function () { M.items.splice(i, 1); renderMake(); };
      row.append(icon(it.is_dir ? "folder" : kindFor(it.name)), el("span", "nm", it.name), x);
      list.appendChild(row);
    });
    var total = M.items.reduce(function (s, i) { return s + (i.is_dir ? 0 : i.size); }, 0);
    var dirs = M.items.filter(function (i) { return i.is_dir; }).length;
    $("m-count").textContent = M.items.length ? plural(M.items.length, "item") + (dirs ? "" : " · " + size(total)) : "";
    $("m-dest").textContent = M.dest ? nicePath(M.dest) : "(pick a folder)";
    validateMake();
  }
  function validateMake() {
    var pwOn = $("m-pw-on").checked, a = $("m-pw").value, b = $("m-pw2").value;
    var hint = $("m-pw-hint");
    if (pwOn) {
      if (S.make.format === "Zip") hint.textContent = "Strong AES-256 lock. Opens in Purrzip, 7-Zip and WinRAR. Windows' and Mac's built-in openers can't open locked ZIPs.";
      else if (S.make.format === "Purr") hint.textContent = "Strong lock. The file names are hidden too, so nobody can see what's inside without the password.";
      else hint.textContent = "Strong AES-256 lock.";
      if (a && b && a !== b) hint.textContent = "The two passwords don't match yet.";
    }
    $("m-hide-row").hidden = S.make.format !== "SevenZ";
    $("m-kind-hint").hidden = S.make.format !== "Purr";
    $("m-level-hint").textContent = S.make.format === "Purr"
      ? "Fast finds repeats but leaves photos as they are. Normal and Smallest also shrink photos; Smallest takes the longest."
      : "Photos, videos and other zips are already squeezed, so they're stored as they are.";
    $("m-repair-row").hidden = S.make.format !== "Purr";
    $("m-go").disabled = !S.make.items.length || (pwOn && (!a || a !== b));
  }
  async function addItems(paths) {
    for (var i = 0; i < paths.length; i++) {
      var p = paths[i];
      if (S.make.items.some(function (x) { return x.path === p; })) continue;
      try { var st = await PZ.call("stat", { path: p }); S.make.items.push({ path: p, name: st.name || baseName(p), is_dir: st.is_dir, size: st.size }); }
      catch (e) { toast("Couldn't read " + baseName(p)); }
    }
    if (S.make.items.length && ($("m-name").dataset.touched !== "1")) {
      $("m-name").value = S.make.items.length === 1 ? S.make.items[0].name.replace(/\.[^.\/]{1,6}$/, "") : "Archive";
    }
    if (!S.make.dest && S.make.items.length) {
      var first = S.make.items[0].path, parent = first.slice(0, Math.max(first.lastIndexOf("/"), first.lastIndexOf("\\")));
      S.make.dest = info.platform === "android" ? (info.downloads || parent) : (parent || info.downloads);
    }
    renderMake();
  }
  function startMake() {
    if (!S.make.dest) S.make.dest = info.downloads || null;
    renderMake();
    show("make");
  }
  $("m-name").addEventListener("input", function () { this.dataset.touched = "1"; });
  $("m-add-files").onclick = async function () {
    var r = await PZ.pick({ multiple: true, title: "Add files" });
    if (r === null) r = await picker("items", S.make.dest, "Add files and folders");
    if (r && r.length) addItems(r);
  };
  $("m-add-folder").onclick = async function () {
    var r = await PZ.pick({ directory: true, title: "Add a folder" });
    if (r === null) r = await picker("items", S.make.dest, "Add files and folders");
    if (r && r.length) addItems(r);
  };
  function segment(id, key) {
    $(id).querySelectorAll("button").forEach(function (b) {
      b.onclick = function () {
        $(id).querySelectorAll("button").forEach(function (x) { x.setAttribute("aria-checked", x === b ? "true" : "false"); });
        S.make[key] = b.dataset.v; validateMake();
      };
    });
  }
  segment("m-format", "format"); segment("m-level", "level");
  $("m-pw-on").onchange = function () { $("m-pw-box").hidden = !this.checked; validateMake(); if (this.checked) $("m-pw").focus(); };
  $("m-pw").oninput = validateMake; $("m-pw2").oninput = validateMake;
  $("m-dest-change").onclick = async function () {
    var d = await chooseFolder(S.make.dest, "Save the archive in which folder?");
    if (d) { S.make.dest = d; renderMake(); }
  };
  $("m-back").onclick = function () { show("home"); };
  $("m-go").onclick = async function () {
    var M = S.make;
    if (!M.dest) { var d = await chooseFolder(null, "Save the archive in which folder?"); if (!d) return; M.dest = d; renderMake(); }
    var pw = $("m-pw-on").checked ? $("m-pw").value : null;
    var opts = { format: M.format, level: M.level, password: pw, hide_names: M.format === "SevenZ" && !!pw && $("m-hide").checked, repair: M.format === "Purr" && $("m-repair").checked };
    var r;
    try { r = await PZ.call("create", { sources: M.items.map(function (i) { return i.path; }), out_dir: M.dest, name: $("m-name").value, options: opts }); }
    catch (e) { return fail("Couldn't start", e); }
    runJob(r.id, "create", {});
  };

  // ---------- jobs ----------
  function runJob(id, kind, extra) {
    S.job = { id: id, kind: kind, extra: extra, started: Date.now() };
    $("w-title").textContent = kind === "create" ? "Packing…" : kind === "repair" ? "Repairing…" : kind === "test" ? "Checking every file…" : "Unzipping…";
    $("w-pct").textContent = "0%"; $("w-bar").firstElementChild.style.width = "0";
    $("w-file").textContent = " "; $("w-bytes").textContent = " ";
    $("w-stop").disabled = false; $("w-stop").textContent = "Stop";
    show("work");
    poll();
  }
  async function poll() {
    if (!S.job) return;
    var j;
    try { j = await PZ.call("job", { id: S.job.id }); }
    catch (e) { S.job = null; return fail("Lost track of the job", e); }
    var pct = j.total ? Math.min(100, Math.floor(j.done * 100 / j.total)) : 0;
    $("w-pct").textContent = pct + "%";
    $("w-bar").firstElementChild.style.width = pct + "%";
    $("w-bar").setAttribute("aria-valuenow", pct);
    if (j.current) $("w-file").textContent = j.current;
    if (j.total) $("w-bytes").textContent = size(j.done) + " of " + size(j.total);
    if (j.status === "running") { setTimeout(poll, 150); return; }
    var job = S.job; S.job = null;
    PZ.call("forget", { id: job.id }).catch(function () {});
    if (j.status === "done") return done(job, j.result);
    if (j.status === "cancelled") {
      toast("Stopped. Nothing half-done was left behind.");
      S.launchQueue = [];
      return show(job.kind === "create" ? "make" : job.kind === "repair" || !S.open ? "home" : "open");
    }
    var err = j.error || {};
    if (job.kind === "extract" && (err.code === "wrong_password" || err.code === "need_password")) {
      show("open");
      var pw = await askPassword(S.open.listing.name, err.code === "wrong_password" ? "That's not it. Try again?" : null);
      if (pw === null) return;
      S.open.password = pw;
      return startExtract(job.extra.only);
    }
    if (job.kind === "repair") return fail("Couldn't repair it", err);
    S.launchQueue = [];
    if (job.kind === "test") {
      if (err.code === "wrong_password" || err.code === "need_password") {
        var pw2 = await askPassword(S.open.listing.name, err.code === "wrong_password" ? "That's not it. Try again?" : null);
        if (pw2 === null) return show("open");
        S.open.password = pw2;
        return startTest();
      }
      return fail("This archive has a problem", err, S.open ? S.open.path : null);
    }
    fail(job.kind === "create" ? "Couldn't make the archive" : "Couldn't unzip it", err, job.kind === "extract" && S.open ? S.open.path : null);
  }
  $("w-stop").onclick = function () {
    if (!S.job) return;
    $("w-stop").disabled = true; $("w-stop").textContent = "Stopping…";
    PZ.call("cancel", { id: S.job.id }).catch(function () {});
  };

  function done(job, res) {
    S.lastResult = { kind: job.kind, res: res };
    var notes = $("d-notes"); notes.textContent = ""; notes.hidden = true;
    if (job.kind === "extract") {
      var rep = res.report;
      $("d-cat").src = "img/done.png";
      $("d-title").textContent = rep.files === 1 && rep.folders === 0 ? "Unzipped!" : "All unpacked!";
      $("d-text").textContent = plural(rep.files, "file") + (rep.folders ? " in " + plural(rep.folders, "folder") : "") + " · " + size(rep.bytes);
      $("d-path").textContent = nicePath(res.folder);
      var skipped = rep.skipped || [], renamed = rep.renamed || [];
      if (skipped.length || renamed.length) {
        notes.hidden = false;
        if (skipped.length) {
          notes.appendChild(el("b", null, "🛡️ " + plural(skipped.length, "item") + " skipped to keep your device safe:"));
          var ul = el("ul"); skipped.slice(0, 50).forEach(function (s) { ul.appendChild(el("li", null, s[0] + " (" + s[1] + ")")); }); notes.appendChild(ul);
        }
        if (renamed.length) {
          notes.appendChild(el("b", null, "✏️ " + plural(renamed.length, "name") + " changed so they fit this device:"));
          var ul2 = el("ul"); renamed.slice(0, 50).forEach(function (s) { ul2.appendChild(el("li", null, s[0] + " → " + s[1])); }); notes.appendChild(ul2);
        }
      }
    } else if (job.kind === "test") {
      var tr = res.report;
      $("d-cat").src = "img/done.png";
      $("d-title").textContent = "All good!";
      $("d-text").textContent = "Every file checked: " + plural(tr.files, "file") + " · " + size(tr.bytes) + ". Nothing is damaged.";
      $("d-path").textContent = nicePath(res.path);
    } else if (job.kind === "repair") {
      var rr = res.report;
      $("d-cat").src = "img/done.png";
      if (rr.clean) {
        $("d-title").textContent = "Nothing to fix";
        $("d-text").textContent = "The repair data says this archive is whole.";
      } else {
        $("d-title").textContent = "Repaired!";
        $("d-text").textContent = "Fixed " + plural(rr.damaged + rr.damaged_repair_data, "damaged spot") + ". The repaired copy is saved next to the original, which was left as it is.";
      }
      $("d-path").textContent = nicePath(res.path);
    } else {
      var r = res.report;
      $("d-cat").src = "img/party.png";
      $("d-title").textContent = "Packed!";
      var saved = r.bytes_in ? Math.round((1 - r.bytes_out / r.bytes_in) * 100) : 0;
      $("d-text").textContent = plural(r.files, "file") + " · " + size(r.bytes_in) + " → " + size(r.bytes_out) + (saved > 0 ? " (" + saved + "% smaller)" : "");
      $("d-path").textContent = nicePath(res.path);
      if (r.skipped && r.skipped.length) {
        notes.hidden = false;
        notes.appendChild(el("b", null, plural(r.skipped.length, "item") + " left out:"));
        var ul3 = el("ul"); r.skipped.slice(0, 50).forEach(function (s) { ul3.appendChild(el("li", null, s[0] + " (" + s[1] + ")")); }); notes.appendChild(ul3);
      }
      if (r.purr && (r.purr.dup_bytes || r.purr.jpeg_count)) {
        // what .purr did beyond ordinary squeezing
        notes.hidden = false;
        notes.appendChild(el("b", null, "What .purr did:"));
        var ulp = el("ul");
        if (r.purr.dup_bytes) ulp.appendChild(el("li", null, "Found " + size(r.purr.dup_bytes) + " of repeats and stored them only once."));
        if (r.purr.jpeg_count) ulp.appendChild(el("li", null, "Shrunk " + plural(r.purr.jpeg_count, "photo") + " from " + size(r.purr.jpeg_in) + " to " + size(r.purr.jpeg_out) + " without changing them."));
        notes.appendChild(ulp);
      }
      S.make.items = []; $("m-name").value = "Archive"; $("m-name").dataset.touched = "";
      $("m-repair").checked = false;
      $("m-pw-on").checked = false; $("m-pw-box").hidden = true; $("m-pw").value = ""; $("m-pw2").value = "";
    }
    $("d-show").textContent = job.kind === "repair" || job.kind === "test" ? "Open it" : "Show it";
    // Android: Purrzip writes files directly (much faster), so it tells the phone's media library what is new.
    var A = window.PurrzipAndroid;
    if (A && A.scan) { try { A.scan(String(job.kind === "extract" ? res.folder : res.path)); } catch (e) {} }
    var c = $("d-cat"); c.classList.remove("pop"); void c.offsetWidth; c.classList.add("pop");
    show("done");
    // several archives picked in the right-click menu: on to the next one
    if (S.launchQueue && S.launchQueue.length) { var nx = S.launchQueue.shift(); setTimeout(function () { launchOne(nx.mode, nx.path); }, 700); }
  }
  $("e-repair").onclick = async function () {
    var path = S.repairPath; if (!path) return;
    var r;
    try { r = await PZ.call("repair", { path: path }); }
    catch (e) { return fail("Couldn't repair it", e); }
    S.open = null;
    runJob(r.id, "repair", {});
  };
  $("d-show").onclick = async function () {
    var r = S.lastResult; if (!r) return;
    if (r.kind === "repair" || r.kind === "test") return openArchive(r.res.path);
    var p = r.kind === "extract" ? r.res.folder : r.res.path;
    if (await PZ.reveal(p)) return;
    var folder = r.kind === "extract" && !r.res.single ? p : p.slice(0, Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\")));
    picker("view", folder, "Your files");
  };
  $("d-again").onclick = function () { show(S.lastResult && S.lastResult.kind === "extract" && S.open ? "open" : "home"); if (S.open) renderOpen(); };
  $("e-back").onclick = function () { show(S.open ? "open" : "home"); if (S.open) renderOpen(); };

  // ---------- home + misc ----------
  $("go-home").onclick = function () { if (!S.job) show("home"); };
  $("home-open").onclick = async function () {
    var r = await PZ.pick({ multiple: false, title: "Open an archive" });
    if (r === null) r = await picker("archive", info.downloads, "Open an archive");
    if (r && r.length) openArchive(r[0]);
  };
  $("home-make").onclick = startMake;
  $("theme").onclick = function () {
    var t = document.documentElement.getAttribute("data-theme") === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", t);
    try { localStorage.setItem("pz-theme", t); } catch (e) {}
    themeIcon();
  };
  function themeIcon() { $("theme").textContent = document.documentElement.getAttribute("data-theme") === "dark" ? "☀️" : "🌙"; }
  themeIcon();
  $("about-btn").onclick = function () { $("about").hidden = false; $("about-ok").focus(); };
  $("about-ok").onclick = function () { $("about").hidden = true; };
  $("about").addEventListener("click", function (e) { if (e.target === $("about")) $("about").hidden = true; });
  document.addEventListener("keydown", function (e) {
    if (e.key !== "Escape") return;
    if (!$("dlg").hidden && dlgResolve) return dlgResolve(null);
    if (!$("pv").hidden) return closePreview();
    if (!$("pick").hidden) return closePicker(null);
    if (!$("about").hidden) $("about").hidden = true;
  });

  // Android Back button: step back inside the app. Returns false on the start screen (the app may close).
  window.pzBack = function () {
    if (!$("dlg").hidden && dlgResolve) { dlgResolve(null); return true; }
    if (!$("pv").hidden) { closePreview(); return true; }
    if (!$("pick").hidden) { closePicker(null); return true; }
    if (!$("about").hidden) { $("about").hidden = true; return true; }
    if (S.job) { return true; } // finish or stop the job first
    if (!$("v-open").hidden && S.open && S.open.cwd) { var c = S.open.cwd; S.open.cwd = c.includes("/") ? c.slice(0, c.lastIndexOf("/")) : ""; renderOpen(); return true; }
    if ($("v-home").hidden) { if (!$("v-done").hidden || !$("v-error").hidden) { show(S.open && $("v-error").hidden === false ? "open" : "home"); } else { S.open = null; show("home"); } return true; }
    return false;
  };

  // Files handed over by the system (Open with…, drag and drop, a second launch).
  async function handlePaths(paths) {
    if (S.job) return toast("Wait for this one to finish first.");
    if (paths.length === 1) {
      try {
        var st = await PZ.call("stat", { path: paths[0] });
        if (!st.is_dir && looksArchive(st.name || "")) return openArchive(paths[0]);
      } catch (e) {}
    }
    S.make.items = [];
    await addItems(paths);
    show("make");
  }
  // Links to web pages (About: Source code) open in the browser
  document.addEventListener("click", async function (ev) {
    var a = ev.target.closest && ev.target.closest('a[target="_blank"]');
    if (!a || !PZ.inApp) return;
    ev.preventDefault();
    if (!(await PZ.openUrl(a.href))) toast("Couldn't open the browser. The address is " + a.href);
  });
  // ---------- right-click menu (Windows): Extract here / to new folder / files…, Test, Add to … ----------
  function parentOf(p) { var i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\")); return i > 0 ? p.slice(0, i) : p; }
  function stemOf(n) { return n.replace(/\.(tar\.(gz|bz2|xz|zst)|[^.]+)$/i, "") || n; }
  async function startTest() {
    var O = S.open;
    if (!(await ensurePassword())) return;
    var r;
    try { r = await PZ.call("test", { path: O.path, password: O.password }); }
    catch (e) { return fail("Couldn't start", e); }
    runJob(r.id, "test", {});
  }
  async function launchOne(mode, path) {
    await openArchive(path);
    if (!S.open || S.open.path !== path) { S.launchQueue = []; return; } // couldn't open, or the password was cancelled
    if (mode === "test") return startTest();
    if (mode === "extract-ask") {
      var d = await chooseFolder(S.open.dest, "Unzip into which folder?");
      if (!d) { S.launchQueue = []; return; }
      S.open.dest = d; $("o-dest").textContent = nicePath(d);
      return startExtract(null);
    }
    S.open.dest = S.open.listing.folder || parentOf(path);
    S.open.ownFolder = mode === "extract-new";
    $("o-dest").textContent = nicePath(S.open.dest);
    startExtract(null);
  }
  async function quickPack(format, paths) {
    S.make.items = [];
    await addItems(paths);
    var dest = parentOf(paths[0]);
    var name = paths.length === 1 ? stemOf(baseName(paths[0])) : (baseName(dest) || "Archive");
    var opts = { format: format, level: "Normal", password: null, hide_names: false, repair: false };
    var r;
    try { r = await PZ.call("create", { sources: paths, out_dir: dest, name: name, options: opts }); }
    catch (e) { return fail("Couldn't start", e); }
    runJob(r.id, "create", {});
  }
  async function runLaunch(mode, paths) {
    if (S.job) return toast("Wait for this one to finish first.");
    if (mode === "add") { S.make.items = []; await addItems(paths); return show("make"); }
    if (mode === "add-purr") return quickPack("Purr", paths);
    if (mode === "add-zip") return quickPack("Zip", paths);
    var list = paths.map(function (p) { return { mode: mode, path: p }; });
    var first = list.shift(); S.launchQueue = list;
    launchOne(first.mode, first.path);
  }
  // Picking several files sends one launch per file: gather them for a moment.
  var launchBuf = null;
  function handleLaunch(l) {
    var mode = l.mode || "open", paths = l.paths || [];
    if (!paths.length) return;
    if (mode === "open") return handlePaths(paths);
    if (launchBuf && launchBuf.mode === mode) { paths.forEach(function (p) { if (launchBuf.paths.indexOf(p) < 0) launchBuf.paths.push(p); }); return; }
    var buf = { mode: mode, paths: paths.slice() };
    launchBuf = buf;
    setTimeout(function () { if (launchBuf === buf) launchBuf = null; runLaunch(buf.mode, buf.paths); }, 900);
  }
  window.pzLaunch = handleLaunch; // for tests
  PZ.onLaunch(handleLaunch);
  PZ.onDrop(handlePaths);
  PZ.onOpenRequest(handlePaths);

  (async function start() {
    try { info = await PZ.call("info"); } catch (e) {}
    try { placesCache = await PZ.call("places"); } catch (e) {}
    if (info.platform && info.platform !== "android") document.documentElement.classList.add("desktop");
    $("about-ver").textContent = info.version ? "v" + info.version : "";
    var A = window.PurrzipAndroid;
    if (A) {
      // Android: Purrzip needs "all files" access to read archives and save unzipped files where you pick.
      var asked = false;
      var askStorage = async function () {
        if (A.hasAllFiles()) return true;
        var ok = await ask({ title: "Let Purrzip see your files", cat: "peek", ok: "Open settings", cancel: "Not now",
          text: "To open archives and save what's inside, Android needs you to switch on \"Allow access to manage all files\" for Purrzip. Purrzip only touches the files you pick, and nothing leaves your phone." });
        if (ok) { asked = true; A.requestAllFiles(); }
        return false;
      };
      window.pzNeedStorage = askStorage;
      askStorage();
      document.addEventListener("visibilitychange", function () {
        if (!document.hidden && asked && A.hasAllFiles()) { asked = false; toast("Thanks! Purrzip can see your files now."); }
      });
      try { var sp = JSON.parse(A.takeStartupPaths() || "[]"); if (sp.length) handlePaths(sp); } catch (e) {}
    }
    try {
      var st = await PZ.call("startup");
      if (st && st.paths && st.paths.length) handleLaunch({ mode: st.mode || "open", paths: st.paths });
    } catch (e) {}
  })();
})();
