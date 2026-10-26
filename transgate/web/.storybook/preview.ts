import type { Decorator, Preview } from "@storybook/svelte";
import "../src/app.css";
import { i18n, LOCALES } from "../src/lib/i18n/index.svelte";
import { palettes } from "../src/lib/theme/palettes";

// Globals the C1 capture drives (web-shell.md section 4):
//   theme   light | dark | system  -> <html data-theme>, absent for system
//   palette generated palette id   -> <html data-palette>, absent for the default
//   locale  one of the sixteen     -> i18n.use(locale) and <html lang>
function text(value: unknown, fallback: string): string {
  return typeof value === "string" ? value : fallback;
}

const applyDocument: Decorator = (story, context) => {
  const root = document.documentElement;
  const theme = text(context.globals.theme, "system");
  const palette = text(context.globals.palette, palettes[0].id);
  if (theme === "light" || theme === "dark") root.dataset.theme = theme;
  else delete root.dataset.theme;
  if (palette !== palettes[0].id) root.dataset.palette = palette;
  else delete root.dataset.palette;
  root.lang = text(context.globals.locale, "en");
  root.dir = "ltr";
  return story();
};

const preview: Preview = {
  loaders: [async ({ globals }) => i18n.use(text(globals.locale, "en"))],
  decorators: [applyDocument],
  globalTypes: {
    theme: {
      description: "Theme",
      toolbar: { title: "Theme", items: ["system", "light", "dark"], dynamicTitle: true },
    },
    palette: {
      description: "Palette",
      toolbar: { title: "Palette", items: palettes.map((p) => ({ value: p.id, title: p.name })), dynamicTitle: true },
    },
    locale: {
      description: "UI locale",
      toolbar: { title: "Locale", items: [...LOCALES], dynamicTitle: true },
    },
  },
  initialGlobals: { theme: "system", palette: palettes[0].id, locale: "en" },
  parameters: {
    layout: "fullscreen",
    a11y: { test: "error" },
    viewport: {
      options: {
        pixel7: { name: "Pixel 7", styles: { width: "412px", height: "915px" }, type: "mobile" },
        desktop: { name: "Desktop", styles: { width: "1280px", height: "800px" }, type: "desktop" },
      },
    },
  },
};

export default preview;
