// Day/Night: follows the device unless the person picked one (remembered on this device only).
(function () {
  var t = null;
  try { t = localStorage.getItem("pz-theme"); } catch (e) {}
  if (t !== "light" && t !== "dark") {
    t = window.matchMedia && matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  document.documentElement.setAttribute("data-theme", t);
})();
