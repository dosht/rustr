// Runs before first paint. Keep in step with src/lib/theme/theme.svelte.ts:
// key "theme" (source ThemeProvider), values light | dark | system.
(function () {
  try {
    var stored = localStorage.getItem("theme");
    if (stored === "light" || stored === "dark") document.documentElement.dataset.theme = stored;
  } catch (e) {
    // Storage blocked: the system theme applies.
  }
})();
