// Talks to the Purrzip engine. In the app (Tauri) it calls the built-in engine; in the
// local test build it calls the test server. Same commands either way.
(function () {
  var T = window.__TAURI__;
  var inApp = !!(T && T.core);
  async function call(cmd, args) {
    if (inApp) {
      try { return await T.core.invoke("pz", { cmd: cmd, args: args || {} }); }
      catch (e) { throw (e && e.code) ? e : { code: "other", message: String(e && e.message || e) }; }
    }
    var r = await fetch("/api/" + cmd, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(args || {}) });
    var j = await r.json();
    if (!j.ok) throw j.error;
    return j.data;
  }
  window.PZ = {
    inApp: inApp,
    call: call,
    fileUrl: function (p) { return inApp ? T.core.convertFileSrc(p) : "/file?path=" + encodeURIComponent(p); },
    // Desktop: the system's own file pickers. Returns null when the in-app browser should be used.
    pick: async function (opts) {
      // Phones use Purrzip's own folder browser (the system picker hands over links, not file paths).
      if (!inApp || !T.dialog || window.PurrzipAndroid || /Android/i.test(navigator.userAgent)) return null;
      var r = await T.dialog.open({ multiple: !!opts.multiple, directory: !!opts.directory, title: opts.title });
      if (r === null || r === undefined) return [];
      return Array.isArray(r) ? r : [r];
    },
    reveal: async function (p) {
      if (inApp && T.opener) { try { await T.opener.revealItemInDir(p); return true; } catch (e) {} }
      return false;
    },
    // Web links open in the normal browser (inside the app a plain link does nothing).
    openUrl: async function (url) {
      if (inApp && T.opener) { try { await T.opener.openUrl(url); return true; } catch (e) {} }
      return false;
    },
    onDrop: function (fn) {
      if (!inApp || !T.webview) return;
      try {
        T.webview.getCurrentWebview().onDragDropEvent(function (ev) {
          var p = ev.payload || {};
          document.body.classList.toggle("dragging", p.type === "over" || p.type === "enter");
          if (p.type === "drop" && p.paths && p.paths.length) fn(p.paths);
          if (p.type === "leave" || p.type === "drop") document.body.classList.remove("dragging");
        });
      } catch (e) {}
    },
    // Right-click menu and second launches: {mode, paths}
    onLaunch: function (fn) {
      if (inApp && T.event) T.event.listen("pz-launch", function (ev) { if (ev.payload) fn(ev.payload); });
    },
    onOpenRequest: function (fn) {
      if (inApp && T.event) T.event.listen("pz-open", function (ev) { if (ev.payload && ev.payload.length) fn(ev.payload); });
      window.pzOpenPaths = fn; // Android hands files over through this
    }
  };
})();
