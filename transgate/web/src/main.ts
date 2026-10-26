// Composition root of the browser shell: host config, shell mechanics (theme,
// consent-gated telemetry), the one Platform object and the Rust presenter.
// Nothing here decides business meaning; App renders core's AppVm.
import "./app.css";
import { mount } from "svelte";
import { loadTransgate } from "$lib/core";
import { documentAssetUrls } from "$lib/documents/assets";
import App from "$lib/features/app/containers/App.svelte";
import ConfigErrorPage from "$lib/features/app/views/ConfigErrorPage.svelte";
import { DEFAULT_LOCALE, i18n } from "$lib/i18n/index.svelte";
import { createBrowserPlatform } from "$lib/platform/browser";
import { loadConfig, type RuntimeConfig } from "$lib/platform/config";
import { createMediaPlatform } from "$lib/platform/media";
import { Telemetry } from "$lib/platform/telemetry.svelte";
import { InvoicePreview } from "$lib/platform/preview.svelte";
import { ThemeStore } from "$lib/theme/theme.svelte";

async function start(target: HTMLElement): Promise<void> {
  let config: RuntimeConfig;
  try {
    config = loadConfig();
  } catch (error) {
    // No network starts without a valid configuration (web-shell.md section 7).
    console.error(error);
    await i18n.use(DEFAULT_LOCALE);
    mount(ConfigErrorPage, { target });
    return;
  }
  const theme = new ThemeStore();
  const telemetry = new Telemetry(config.telemetry, () => i18n.locale);
  const media = createMediaPlatform();
  const preview = new InvoicePreview();
  const browser = createBrowserPlatform({ telemetry });
  const core = await loadTransgate(
    {
      apiUrl: config.apiUrl,
      authUrl: config.authUrl,
      authKey: config.authKey,
      siteUrl: config.siteUrl,
      documentAssets: documentAssetUrls,
      // Sign-in submit waits for a Turnstile token only when a site key is configured.
      captcha: config.captcha !== null,
    },
    { ...media, ...browser, preview: (bytes, filename, mime) => preview.present(bytes, filename, mime) },
  );
  preview.sync(core.current());
  mount(App, { target, props: { core, media, theme, telemetry, preview, copy: browser.copy, captcha: config.captcha } });
}

const target = document.getElementById("app");
if (!target) throw new Error("index.html has no #app");
void start(target);
