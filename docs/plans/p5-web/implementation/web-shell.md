# P5 web shell contract (p1V)

Owner: w23:p1V (Opus), preserved saved session. Slice: the `transgate/web` package, the app frame, auth, settings and commerce views, shared accessible UI primitives, theme and route containers, Storybook and build configuration, and the delegated root token generator outputs and roles.

Status: **source-only contract**, published before any dependent code. Nothing has been built, run, linted, formatted or opened in a browser. Views that depend on unpublished Rust view models are not written yet; there are no guessed stubs.

## 1. Package (answers to p1Y question 1)

- **Path:** `transgate/web`, package name `transgate-web`, ESM, Svelte 5 with Vite.
- **Scripts:** `dev`, `check` (svelte-check), `build` (check then `vite build`), `preview`, `test` (vitest), `test:ci` (`vitest run`), `storybook` (port 6007), `build-storybook`, `e2e` (`playwright test`). `gate-transgate.sh` calls `test:ci` and `e2e`.
- **Lockfile:** `transgate/web/package-lock.json`. It is generated in the lead-authorized integration window (p1Y or lead), because no worker may run npm. Until then `package.json` is source only.
- **Owned by others, inside this package:**
  - p1Y: `playwright.config.ts` and `e2e/**`. I write neither.
  - p22: `src/lib/core/{index.ts,types/*.ts,fixtures.json,pkg/*}`, the presenter facade and generated outputs.
  - p24: `locales/<code>/<ns>.json` and its manifest and provenance. Its importer and validator live in `scripts/p5-locales`.
  - p1W: font assets for documents, location to be named by p1W.
  - Feature view owners: their own `src/lib/features/<feature>/**` folders (p1Z workspace, p21 AI). They import shared primitives from `src/lib/ui`, the theme from `src/lib/theme` and the device class from `src/lib/platform`.
- **Runtime config:** none baked at build time beyond what p1T decides (host-injected versus build-time); p1Y forwarded that question to p1T. `vite.config.ts` proxies `/api` and `/checkout` to the host and `/auth/v1` to GoTrue in dev only.

## 2. Storybook (answer to question 2)

- **Location:** `transgate/web/.storybook`. Stories match `src/**/*.stories.ts`.
- **Titles:**
  - feature views: `Features/<Feature>/DesktopViews/<View>` and `Features/<Feature>/MobileViews/<View>`;
  - page-level containers: `Pages/App/<Name>`;
  - shared primitives: `UI/Primitives/<Name>`.
- **Data:** feature and page stories render only generated Rust fixtures from `src/lib/core/fixtures.json`. A story never hand-writes a view model; primitive stories use plain props. `src/lib/stories.test.ts` renders every story in Vitest, the taskr pattern.
- **Addons:** `@storybook/addon-a11y` with `a11y.test: "error"`, and `@storybook/addon-docs`.

## 3. Locator contract (answer to question 3): accepted, with precision

- Use role plus accessible name in the `en` locale. Names come from the `en` catalogs, never from hard-coded literals.
- Dialogs and sheets are native `<dialog>` with `role=dialog` (implicit) and a name from their visible title (`aria-labelledby`).
- Switches use `role=switch`. Tabs use `tablist`, `tab` and `tabpanel`. Async outcomes use `role=status`; errors and blocking alerts use `role=alert`.
- Menus are disclosure buttons (`aria-expanded`) over a list of links or buttons, not ARIA `menu` widgets.
- `data-testid` appears only where no semantic role exists, such as a waveform canvas or a player time readout. Each one is listed in the owning view's handoff.
- Non-`en` locale runs locate by role and by the same key's translated name from the catalog, never by the English string.

## 4. Theme and phone layout triggers (answer to question 4)

**Theme.** This is a shell-local mechanic. p22 agreed that it is not a Rust port.

- **Values:** `light`, `dark` or `system`, stored in `localStorage["theme"]` (source key, `ThemeProvider.tsx:52-60`). The default is `system` (`ThemeProvider.tsx:11`).
- **Application:** `light` or `dark` sets `document.documentElement.dataset.theme`. `system` removes the attribute, so the generated CSS media query applies. A pre-paint inline script in `index.html` applies the stored value before first paint.
- **Storybook:** a `theme` toolbar global (light, dark, system) sets the same attribute. For `system`, C1 also sets the browser `colorScheme` (Playwright `emulateMedia`).
- **Palette:** a `palette` toolbar global sets `data-palette`. The default attribute is absent, so Paper Moss applies.

**Phone layout.**

- **Stories:** desktop and phone are separate pure views with separate stories (`DesktopViews` versus `MobileViews`). No device switch is needed for them. Phone stories set the Storybook viewport to a Pixel 7-sized phone.
- **`Pages/App` stories:** they take an explicit `device: "desktop" | "phone"` arg passed to the container, bypassing detection.
- **Browser (C2/C3):** phone layout means a touch device (`'ontouchstart' in window || navigator.maxTouchPoints > 0`), `innerWidth < 1024` (`useDeviceType.ts:21,79-92`), and the mobile flags from the session capabilities. The Playwright Pixel 7 project (`hasTouch`) triggers it; Desktop Chrome does not.
- **Re-evaluation:** the shell re-evaluates on `resize`, and the presenter instance survives a branch switch.

**Landscape notice.** This follows the source rotation rule (`useIsPhoneLandscape.ts`): screen edges, not the viewport, so an open keyboard does not trigger it. When shown, it is an accessible modal notice and the app beneath is `inert`. It replaces the source's `aria-hidden` overlay that left the app focusable underneath (D9).

## 5. Tokens (answer to question 5)

- **New generator outputs:**
  - `transgate/web/src/tokens.css`, using the same CSS shape as `apps/web/src/tokens.css`;
  - `transgate/web/src/lib/theme/palettes.ts`.
- **New semantic roles:** `success` and `progress` in `design-tokens.json`, for all three palettes in both modes. Contrast checks are text-grade (at least 4.5:1) against `bg`, `surface`, `cursor-bg` and `notice-bg`.
- **Effect on taskr outputs:** regeneration also rewrites the taskr Kotlin, Swift and TUI outputs, which iterate the token list. Those gain the two fields with no taskr behavior change.
- **Edit window:** I edit `scripts/gen-tokens.py` and `design-tokens.json` only in the window coordinated with p1Y. The regeneration itself runs there.

## 6. Pending inputs (no dependent code until received)

| From | Needed |
|---|---|
| p1X | App, session, route, settings, feedback, support and checkout view models and intents; `Msg {ns,key,args}` shape |
| p22 | Final `TransgateCore`, `AppIntent`, `AppView` and `Platform` names and signatures (preview received; not yet final) |
| p1R | Purchase, invoice and subscription view models, including `display_amount` versus the charged quote and the checkout handoff data |
| p24 | i18next init options used by the validator, and the catalog manifest path and format |
| p1Y | Token edit window |

## 7. Host contract for p1T: index, runtime config, assets, CSP

**Runtime config is injected by the host at serve time, not baked into the build.** One image serves every deployment.

**Injection point.** `dist/index.html` keeps the exact marker `<!--tg-inject-->` inside `<head>`. For every app-route document response, the host replaces it with:

```html
<script type="application/json" id="tg-config">{…}</script>
```

On the browser `POST /checkout/confirm` response, it also adds `<script type="application/json" id="tg-confirm">{ConfirmOutcome}</script>`: p1R's single trusted serde-tagged result, exactly `{"outcome":"confirmed","view":ConfirmView}` (a pending payment stays pending inside `view`) or `{"outcome":"failed","status":<u16>,"code":"…","message":"…"}`. Never a raw `ConfirmView`. The shell validates only the tag and hands the envelope to core unchanged; core decides what it means.

**Escaping.** JSON with `<`, `>`, `&`, U+2028 and U+2029 written as `\u003c`, `\u003e`, `\u0026`, `\u2028` and `\u2029`.

**`tg-config` fields.** All public, no secrets.

```json
{
  "apiUrl": "/api",
  "authUrl": "/auth/v1 or the absolute GoTrue base",
  "authKey": "GoTrue anon/publishable key or null",
  "siteUrl": "https://app.origin",
  "captcha": { "provider": "turnstile", "siteKey": "…" } | null,
  "telemetry": {
    "posthog": { "key": "…", "host": "…" } | null,
    "sentry": { "dsn": "…", "environment": "…" } | null
  }
}
```

- **Not in config:** feature flags and capabilities come only from `GET /api/session`.
- **Telemetry:** it initializes only after cookie consent (D12).
- **Missing config:** in a production build, a missing or invalid `tg-config` renders a visible configuration-error page and starts no network calls. In `vite dev` only, the shell reads `VITE_TG_*` env values instead, since the dev server has no host.

**Assets (Vite `build`, P5 D20).** Every app-owned asset is under `/_app/*`; `/assets/*` stays the source public path owned by P6, with no alias or catch-all. `vite.config.ts` keeps the default `base` `/` and `outDir` `dist`, sets `build.assetsDir: "_app"` (a flat directory of `<name>-<hash>.<ext>` files) and `assetsInlineLimit: 0`, so no import becomes a `data:` URL. The URL of a file is `/` plus its path under `dist`.

| Asset | Path | Cache |
|---|---|---|
| Hashed JS, CSS, wasm (`new URL(..., import.meta.url)`), licensed fonts and notices, invoice logo, rating artwork (`?url`) | `dist/_app/*` | `immutable` |
| Pre-paint theme script (`public/_app/theme-init.js`, the only `public/` file) | `dist/_app/theme-init.js` | `no-cache` |
| Document | `dist/index.html` | `no-cache` / `no-store` |

- Document fonts (base 4 + D18 fallbacks for Hebrew, Armenian, Georgian, Devanagari, Kannada, Tamil, Thai and pan-CJK), every SIL OFL 1.1 notice, the CJK copyright notice and the invoice logo have one copy in `transgate/export/{fonts,assets}` (p1W). `src/lib/documents/assets.ts` imports them with `?url` only (no bytes in JS), so Vite emits them hashed into `dist/_app`; its `documentAssetUrls` is p22's `TransgateConfig.documentAssets`, and `fontLicenses` lists the notice URLs. Nothing is fetched at page load: Rust fetches the base set on the first PDF and a fallback family only when a document needs it (the 16.5 MB CJK face never enters the initial payload). `vite.config.ts` allows `../export` in dev. The UI itself uses the system font stack.
- Wasm must be served as `application/wasm`.

**App routes.** The exact list is X's `Route`. The host fallback serves `index.html` only for those paths and their 15 locale prefixes (`/{tr|pt|…}/…`), never for `/api/*` (JSON 404) or for P6 public paths.

**CSP baseline for app documents:**

```
default-src 'self';
script-src 'self' 'wasm-unsafe-eval';
style-src 'self';
img-src 'self' data: blob:;
font-src 'self';
media-src 'self' blob:;
connect-src 'self';
frame-ancestors 'none';
base-uri 'self';
form-action 'self';
```

Extend it only from configuration:

- **GoTrue:** its origin in `connect-src` when it is not same-origin. Google OAuth is a top-level navigation, so it needs no CSP source.
- **Turnstile** (when configured): `https://challenges.cloudflare.com` in `script-src` and `frame-src`.
- **Telemetry** (when configured): the PostHog host and Sentry ingest host in `connect-src`, plus the PostHog host in `script-src` (posthog-js lazy-loads its extension scripts from its host). The SDKs themselves are bundled same-origin chunks that load only after consent; before it, no telemetry request or script load happens.
- **Storage:** origins derived by the host from the configured endpoint/public bucket URL, in `connect-src` (signed `PUT`) and `media-src` (signed `GET`). No separate CSP env.
- **YouTube player:** `https://www.youtube.com` in `script-src` (runtime iframe API, no npm dependency) and `frame-src`; thumbnails in `img-src`.

**Checkout (P5 D17).** The provider form never runs in the app document. The purchase flow (Rust, p22) submits one hidden same-origin `POST /checkout` form (fields in p1R's billing.md section 6); the host answers a separate, non-SPA checkout document with its own nonce and Iyzico CSP (p1T), LemonSqueezy is a 303 to the provider, and `GET /checkout` still redirects. The app document gets no checkout nonce, no Iyzico origins and no `style-src 'unsafe-inline'`. Markup: section 9.

## 8. Conventions for feature view owners (p1Z, p21, p1R)

**Directories.** `src/lib/features/<feature>/views/desktop/*.svelte`, `views/mobile/*.svelte`, `containers/*.svelte`, stories and tests beside the component (`X.stories.ts`, `X.test.ts`). Feature folders: `workspace`, `history`, `transcript` (p1Z), `ai` (p21). Mine: `app` (frame, landscape notice, cookie consent), `auth`, `settings`, `commerce` (purchase, checkout handoff/confirm, invoices), and `feedback` + `support` (desktop and phone modal views; list rows and the transcript only render the trigger that dispatches p1X's intent).

**Primitives** (`$lib/ui/*.svelte`; labels are always props, already translated by the caller; no primitive calls i18n):

| Component | Props (summary) | Role |
|---|---|---|
| `Button` | `variant` primary/secondary/ghost/danger, `size` sm/md, `busy`, `type`, `disabled`, `onclick`, children | button; `busy` sets `aria-busy`, ignores clicks |
| `IconButton` | `label` (required accessible name), `icon` (a `@lucide/svelte/icons/*` component), `pressed?`, `size` | button, `aria-pressed` when given |
| `Dialog` | `open`, `title`, `onclose`, `kind` dialog/sheet (sheet docks to the bottom on phones), `wide?` (desktop commerce/PDF), `locked`, children, `actions` snippet | native `<dialog>` named by its title |
| `ConfirmDialog` | `open`, `title`, `description`, `confirmLabel`, `cancelLabel`, `tone` danger/neutral, `busy`, `kind`, `onconfirm`, `oncancel` | dialog; no dismissal while busy |
| `Switch` | `checked`, `label`, `description?`, `disabled`, `onchange(requested)` | `role=switch`; the owner decides |
| `Tabs` | `label`, `tabs[{id,label}]`, `selected`, `onselect`, `panel` snippet | tablist/tab/tabpanel, arrows/Home/End |
| `Menu` | `label`, `items[{id,label,disabled?}]`, `onselect`, `trigger?` snippet, `align` | disclosure button + list of buttons |
| `TextField` | `label`, `value`, `oninput(value)`, `hint?`, `error?`, `multiline?`, native input/textarea attributes | labelled field, `aria-invalid`/`aria-describedby` |
| `Select` | `label`, `value`, `options[{value,label,disabled?}]`, `onchange`, `hideLabel?` | native select |
| `Progress` | `label`, `value?` (0-100, omit = indeterminate), `showValue` | progressbar |
| `Badge` | `tone` neutral/progress/accent/success/danger (core `Tone`), `spinner`, children | text |
| `Alert` | `tone` info/success/error, `title?`, children, `actions?`, `ondismiss?` + `dismissLabel` | `status`, `alert` for error |
| `Skeleton` | `label`, `lines` | `status` + `aria-busy` |
| `SplitPane` | `separatorLabel`, `primary{defaultSize,minSize,maxSize?}`, `secondary{...,collapsedSize?}`, `collapsed?`, `oncollapsedchange?`, `first`/`second` snippets | paneforge separator with `aria-valuenow` |

Icons: `@lucide/svelte`, imported per icon (`import X from "@lucide/svelte/icons/x"`), always `aria-hidden` inside a named control. Colors only through the semantic Tailwind utilities in `src/app.css` (`bg-surface`, `text-muted`, `border-border`, `text-error`, `bg-progress`, ...), never hex.

**i18n** (`$lib/i18n/index.svelte`): `i18n.t(msg)` for core `Msg {ns,key,args}` (nested Msg arguments are translated first); `i18n.tk(ns, key, args?)` for view-owned static labels. Keys are only those in p24's catalogs and `reachable-keys.json`; request missing keys from p24, never add literals. There is no runtime fallback language; in dev and tests a missing key throws. Text is rendered through Svelte text/attributes only (never `{@html}`).

**Stories.** Titles as in section 2. Feature stories import generated fixtures (`import fixtures from "$lib/core/fixtures.json"`) by name; no hand-written view models. Globals `theme`, `palette` and `locale` are applied by `.storybook/preview.ts` (the loader calls `i18n.use(locale)`, so stories load real catalogs). Phone stories set `parameters.viewport.defaultViewport = "phone"`. `Pages/App` stories pass the core-decided layout from the fixture, not a device arg.

**Tests.** Vitest + jsdom + `@testing-library/svelte` + `@testing-library/user-event`; `src/test/setup.ts` provides `matchMedia` and `<dialog>` in jsdom. Component tests per platform view (`DesktopX.test.ts`, `MobileX.test.ts`) assert user-visible behavior and emitted intents. The old generic `stories.test.ts` only asserted a nonempty render root and is removed; Y's real Storybook C1 render remains required, not replaced by this incidental assertion.

**Device/layout.** Containers render core's `AppVm` layout decisions (p1X `layout.rs`: frame, transcript and YouTube layouts plus `phone_ai`, from `Viewport {touch,width}` and session flags). The presenter measures the initial viewport itself at start (p22); the app container dispatches `AppIntent::ViewportChanged { viewport }` only on later changes, via `$lib/platform/viewport.svelte.ts` (`watchViewport`, coalesced per frame, change-only). No TS breakpoints or flag checks in views or containers. The landscape notice is shell-local (`$lib/platform/landscape.ts`, source screen-edge rule).

**Platform object.** Assembled once in `src/main.ts`: media from p1Z; `copy`, `save`, `share`, `track` and `resetTelemetry` from `$lib/platform/browser.ts`; `preview(bytes, filename, mime)` delegates to the root-scoped `InvoicePreview` in `$lib/platform/preview.svelte.ts`. `App.svelte` synchronizes its selection before exposing each new VM and disposes both core and preview on unmount.

## 9. Checkout document markup for p1T's `CheckoutPage` (D17)

Two static files, read by the host renderer (for example `include_str!`), not processed by Vite: `transgate/web/checkout/document.html` and `transgate/web/checkout/failure.html`. Source: `pages/checkout/index.tsx:88-127`. Always light (`data-theme="light"`), as the source forced white.

`document.html` placeholders, each replaced exactly once:

| Placeholder | Value | Escaping |
|---|---|---|
| `{{lang}}` | the outcome's `locale` (`Form.locale`, `Failed.locale`, else the default `en`) | HTML attribute |
| `{{title}}` | `common` key `Checkout` in that locale | HTML text |
| `{{tokens_css}}` | `transgate/web/src/tokens.css` verbatim (generated, static) | none |
| `<!--tg-checkout-body-->` | `Form`: the provider's trusted `content`, raw. `Failed`: `failure.html` filled in | as below |

`failure.html` placeholders, all HTML text-escaped, in the same locale: `{{heading}}` = `common` `We couldn't start your checkout`; `{{message}}` = the sign-in message when `status` is 401, otherwise the generic "nothing was charged" message (source `index.tsx:96-99`; the two keys are requested from p24); `{{back_label}}` = `common` `Back to plans`. The link target is the source's `/#pricing`. Copy comes from p24's catalogs, never from `Failed.message`, so no provider or internal text reaches the page.

Policy: P5 D19's checkout-only CSP. The template's own `<style>` carries no nonce and relies on D19's `style-src https: 'unsafe-inline'`; the renderer must not add a style nonce, which would make browsers ignore `'unsafe-inline'` for the provider's styles. The document has no script of its own; the per-response script nonce goes only on the provider form's scripts. The source "Güvenli Ödeme" footer strip (`https://cpp.iyzipay.com`, shown on both outcomes) is covered by D19's `img-src https:`. No app script, wasm, `tg-config` or telemetry runs here.

## 10. Source handoff (p1V)

**Status: source only.** No build, test, lint, formatter, browser or provider run by this owner. Nothing below is claimed passing.

**Continuation provenance (2026-10-26):** user explicitly authorized Astra high to continue this same preserved session and original P5 web ownership. Earlier Opus source and evidence remain attributed to that work; frozen advisor documents are unchanged. This continuation implements the owed frame/commerce integration from Y's explicitly released corrected run5 R/X outputs. No Rust, generated output, package/lock, checkout include input, canonical gate or advisor-file edits; no checks, browser/provider runs, or new agents by this owner.

**Written (mine):**

- Package and config: `transgate/web/{package.json,vite.config.ts,svelte.config.js,tsconfig.json,index.html,.gitignore}`, `public/_app/theme-init.js`, `.storybook/{main,preview}.ts`, `src/{main.ts,app.css,vite-env.d.ts,test/setup.ts}`. Runtime deps: `@lucide/svelte`, `@sentry/browser`, `i18next`, `paneforge` 1.0.2, `posthog-js`, `wavesurfer.js` (p1Z). Lockfile is p1Y's. D20: all app assets under `/_app` (section 7).
- Shell mechanics: `src/lib/i18n/index.svelte.ts`; `src/lib/theme/{theme.svelte.ts,ThemeToggle.svelte}`; `src/lib/platform/{config.ts,telemetry.svelte.ts,browser.ts,viewport.svelte.ts,landscape.ts}`; `src/lib/documents/assets.ts` (URL-only D18 map = `TransgateConfig.documentAssets`).
- Primitives `src/lib/ui/*` (section 8 table) with stories.
- Root and frame: `features/app/containers/{App,SignedInFrame}.svelte` (AppVm state switch; desktop sidebar or phone header/drawer/account sheet by `layouts.frame`; p1Z's `SidebarHistory`/`HomeContent` and p21's `AiConsentSettings` mounted with the agreed props); `features/app/views/{desktop/{DesktopSidebar,UserArea},mobile/{PhoneHeader,PhoneDrawer,AccountSheet},CookieConsent,LandscapeNotice,ConfigErrorPage}.svelte`; source logos copied unchanged to `features/app/art/` (`logo.png` f1e69df2…, `logo_0.png` b9eadec8…).
- Auth: `features/auth/views/{AuthView,NewPasswordView,Turnstile}.svelte`.
- Settings: `features/settings/views/{SettingsDialog,ProfilePanel,ManageDataPanel}.svelte`.
- Feedback/support: `features/feedback/views/{FeedbackDialog,desktop/DesktopFeedback,mobile/MobileFeedback}.svelte` (+ source rating art in `features/feedback/art/`, unchanged copies of the source `public/assets/{disappointed,satisfied,outstanding}.svg`, SHA-256 6ee8f87b…, e653d5f5…, a341d6bc…); `features/support/views/{SupportDialog,desktop/DesktopSupport,mobile/MobileSupport}.svelte`.
- Commerce: `features/commerce/confirm.ts` (tg-confirm envelope); `views/{PurchaseDialog,SubscriptionPanel,InvoicesPage,ConfirmPage,SpecialOffer}.svelte`; pure `desktop/Desktop{Purchase,Subscription,Invoices}.svelte` and `mobile/Mobile{Purchase,Subscription,Invoices}.svelte`; generated-fixture stories and per-platform behavior tests. `platform/preview.svelte.ts` owns PDF URLs. Checkout document markup `transgate/web/checkout/{document,failure}.html` remains unchanged in this continuation (section 9), consumed by p1T.

**Regression sources:** existing `src/lib/{i18n/index,theme/theme,platform/config,platform/telemetry,platform/browser,platform/viewport,platform/landscape,ui/primitives}.test.ts`; feedback/support/auth/frame component tests; new `platform/preview.test.ts`, `features/commerce/views/{ConfirmPage,SpecialOffer}.test.ts`, and six `views/{desktop,mobile}/*{Purchase,Invoices,Subscription}.test.ts` files. All new tests are source only. Y owns application checks, Storybook/browser verification and the complete generated wasm seam.

**Generated publication and consumer integration:**

- Y explicitly released corrected run5, not the rejected run2 physical files. Read `/tmp/rustr-p5-tsgen/run5/report.json`: command exit 0, compile PASS, 3 generator tests, 144 TS files, 21 fixture roots, corrected analysis-field inspection, publication READY. Generated manifest SHA-256 `5ff92934aa5daa730a35e6a7fd694c2a43dbeb40d6715bccc0e231edbfa2bb28`; fixture SHA-256 `602865cdf1c9da5786b5234a8287f036912d09a91715e1ef40aa6eb31a9137b2`.
- Owned consumers now mount purchase, subscription, invoice history, optional-status confirmation, and special offers from those generated contracts. No hand-written story VMs. `SignedInFrame.stories.ts` consumes all six composed frame fixtures.
- p1Z reported `SidebarHistory.svelte` saved and `HomeContent.svelte` still in progress. Those imports retain the agreed names/props; no replacement or overlapping edits by p1V. Their readiness and Y's diagnostics remain outside this source handoff.

**Catalog prerequisite cleared (source only):** p24 published all16 catalogs and reachable/provenance manifests, including the exact frame/theme/config keys and common `Email: ` / `Joined on: ` keys with trailing spaces. See `implementation/locales.md` sections 5–6. D22 remains X/p21's contract: settings DELETE turns off AI and deletes saved account-side data; modal POST-false refusal preserves completed results; `cleanup_pending` still means deleting. This clears the catalog-publication blocker, not runtime localization verification.

**Next handoff: p1Y diagnoses the completed owned rendering source when the remaining shared web inputs are coherent and the lead authorizes the checkpoint.** No build, unit-test, browser, PDF-viewer or localization pass is claimed here. All S1/compiled-input holds remain in force.

**Recorded choices:** telemetry SDKs load only after consent (stricter than the source, D12); Sentry keeps the source scrubbing and masked replay but no performance tracing; PostHog never starts for the `id` UI locale (source); the support dialog keeps a failed message (UI-04 in core); the landscape notice is a modal dialog so the app beneath is inert (D9); the sidebar account menu omits "API Keys" until P7 (D13); the KenzNote promo is not carried into the app (screens ledger open question); the sign-in form is shown beside the Turnstile widget and core keeps it unsubmittable until solved (the source hid the form until then); manual pageviews per route as in the source.

### Bounded lock conflict repair (2026-10-26)

Y clarified that the lead opened the PRE-S1 R/X/22 completion window and no S1 snapshot exists yet; the earlier frozen-snapshot wording above described a constraint, not an existing artifact. Package edits remain held except for this explicitly authorized conflict repair.

Y's failed isolated resolution is preserved at `/tmp/rustr-p5-node24-lock-xwep1ocn/resolve-failure.txt`: Node 24.22.1/npm 11.19.1, original manifest SHA-256 `9518c7d371baaec8abc1ba6476d2a54806ad2d468a6121f078e56610e0a523ad`, ERESOLVE because i18next 24.2.3 requires optional TypeScript peer `^5`, while the root requested `~6.0.3`. No lock was emitted.

Repair: change only `devDependencies.typescript` to `~5.9.3`; retain i18next `^24.2.3` and all other dependencies. Published metadata permits TypeScript 5: [svelte-check 4.7.6](https://registry.npmjs.org/svelte-check/4.7.6) and [svelte2tsx 0.7.61](https://registry.npmjs.org/svelte2tsx/0.7.61) both accept it; [Storybook Svelte-Vite 10.6.2](https://registry.npmjs.org/@storybook%2fsvelte-vite/10.6.2) itself depends on TypeScript `^4.9.4 || ^5.0.0`. This repairs the declared conflict without changing the runtime translation library or bypassing peers.

Repaired manifest SHA-256: `977f23df879f792108ee9d37ecc5f5d81bbd21e5870e1eff895b7695980e125f`. Source ready for Y's fresh isolated lock resolution and `npm ci`; neither has been executed by p1V. Full resolution and application type compatibility remain unverified. No force, legacy-peer-deps, overrides, app checks or build.

Y's second attempt accepted the TypeScript correction but failed on mixed Storybook patches: `storybook@10.6.3` versus `@storybook/addon-a11y@10.6.4` requiring `storybook ^10.6.4`. Full report/debug log remain at `/tmp/rustr-p5-node24-lock-xwep1ocn/attempt2/npm-cache/_logs/2026-10-26T10_35_35_146Z-{eresolve-report.txt,debug-0.log}`. No lock was emitted.

Second bounded repair: pin all five direct Storybook entries to exact `10.6.2`: `storybook`, `@storybook/addon-a11y`, `@storybook/addon-docs`, `@storybook/svelte`, `@storybook/svelte-vite`. All five release metadata records were read from npm; the four adapters/addons accept core `^10.6.2`. Sources: [core](https://registry.npmjs.org/storybook/10.6.2), [a11y](https://registry.npmjs.org/@storybook%2faddon-a11y/10.6.2), [docs](https://registry.npmjs.org/@storybook%2faddon-docs/10.6.2), [Svelte](https://registry.npmjs.org/@storybook%2fsvelte/10.6.2), [Svelte-Vite](https://registry.npmjs.org/@storybook%2fsvelte-vite/10.6.2). Exact pins prevent direct family patch skew without a version upgrade or resolver override. TypeScript remains `~5.9.3`; all other entries remain unchanged.

Current manifest SHA-256: `52bf05e3c957304fd0026120f64a4b9202423a3a5f96cad0d40273e066f486dd`. Source ready for Y's next isolated resolution; package edits held again. Registry constraints are evidence of declared compatibility, not a passing install or application check. None executed by p1V.

**Lock window closed successfully (Y-reported):** attempt3 resolved this final manifest using pinned Node 24.22.1/npm 11.19.1; a fresh isolated `npm ci` added 231 packages and exited 0. Published `transgate/web/package-lock.json` SHA-256 is `10c9b625b8f7d13c3ba1baa448d76568ba9971d5f0bb0ad4f3cc7920af2dcbc8`, containing 354 entries including optional platforms. Installed versions: TypeScript 5.9.3, all five Storybook packages 10.6.2, i18next 24.2.3, Svelte 5.57.2, Vite 8.3.5, Playwright 1.64.1. Evidence: `/tmp/rustr-p5-node24-lock-xwep1ocn/attempt3`.

Y reported unapproved core-js/esbuild install-script warnings; no approval or override was used. Offline `esbuild --version` succeeded with 0.28.2. This closes dependency resolution/install only: no app check, build, Storybook or browser proof. p1V did not rerun these commands. Package edits may resume only with coordination; dependency changes require Y's lock refresh.

**X composition publication (source, export pending):** `AppVm::CheckoutConfirm { locale, vm: Option<ConfirmVm>, messages: Option<ConfirmMessages> }` uses `vm: None` for loading. `SignedInVm` adds `invoices`, `billing_messages`, and `special_client_alert`. Invoice actions are `Preview { payment_id }`, `ClosePreview`, `Download { payment_id }`, `LookupEmailTyped { text }`, `LookupRequested`, `LookupCleared`, and `TemporaryBillToTyped { text }` under `AppIntent::Invoices`. Billing messages cover purchase/coupon/renewal/subscription/invoice failures and customer-not-found; `renewal_at_ms` permits locale date formatting only, never arithmetic. Confirm messages are `title`, `status`, `message`, `detail`, and `redirect`. Host alert text stays verbatim, with UI-local dismissal per R section 10. X will fold R's fixture keys unchanged into the export alongside AppVm/SignedInVm. These published source contracts do not replace generated types or fixture data; integration remains pending those outputs.

**Final fixture/export source publication (X report; generation pending):**
- R keys remain `PurchaseVm`, `SubscriptionVm`, `InvoicesVm`, `ConfirmVm`, with unchanged R case names including `document_failed`.
- `ConfirmMessages` uses matching confirm case names. Its `status` is `Option<Msg>`: absent for unconfirmed, where `message` carries the warning once.
- `BillingMessages` uses `purchase_<Rname>`, `subscription_<Rname>`, and `invoices_<Rname>`; it includes `document_failure: Option<Msg>`. Stories consume these generated copy fixtures, not hand-written message VMs.
- `SignedInVm`: `desktop_new`, `phone_job`, `settings_open`, `billing_owner`, `purchase_open`, `special_offer`.
- `AppVm`: those six names plus `restoring`, `signed_out`, `erased`, `confirmation_succeeded`, `confirmation_declined`, `confirmation_pending`.

This supersedes “fixture/export completion next” in the earlier source relay: source is now written, but no generated-output or render claim is made until Y runs generation.

### Invoice preview capability published by p22

`Platform.preview(bytes: Uint8Array, filename: string, mime: string): void` consumes the existing Rust invoice PDF bytes. Synchronous Blob/object-URL failures propagate like `save`. Preview is an embedded invoice-modal iframe, never a download or a new window. p1V owns the root-scoped reactive URL resource and revokes it on replacement, close, leaving invoices, session loss and root disposal. p22 owns the port/driver and must reject stale PDF completions before invoking it using session, invoice context, latest preview request and current open selection; closing/reopening the same payment must not admit the old request. The proposed port carries no payment identity, so the shell cannot enforce that driver fence.

Host prerequisite cleared at source level (p1T report): ordinary app/confirmation `frame-src` now permits `blob:` alongside YouTube/configured Turnstile. `script-src` still excludes Blob URLs, `object-src 'none'` remains, and checkout D19 is unchanged. p1T updated the existing CSP regression source and `hosts.md`; no checks ran. Actual embedded PDF viewer behavior remains Y-owned browser proof, not established by this source change.

Confirmation analytics use existing consent-gated `Platform.track`: `PaymentCompleted` → `payment_completed`, `PaymentFailed` → `payment_failed`, `SubscriptionStarted` → `subscription_started`, with empty properties. No GA/conversion hook exists. The source's GA `Purchase` intent is omitted under D12's explicit GA exclusion, not replaced with an invented PostHog purchase event. Server transactional analytics remains unchanged. This records the agreed transport boundary, not an implemented driver or rendered preview.

p22 published `Platform.preview` in `src/lib/core/platform.ts` (read by p1V); the Rust binding/wrapper catches synchronous failures per p22's report. Confirmation analytics mapping is saved, and `ReadConfirmation` reads only `#tg-confirm` JSON, with no endpoint or authentication request. Preview delivery is now wired by p22: immediately before `Platform.preview`, the driver checks disposal, `AppState::is_current(epoch)`, and `is_current_invoice_preview(ticket, payment_id)`, using the originating effect's ticket rather than rereading it. Capture/render/font/preview/save errors emit `DocumentFailed { payment_id, failure: Unavailable }` with that ticket; stale failures cannot clear a newer preview. Successful previews emit no result event. Downloads remain session/disposal-fenced. This is p22-reported source evidence, not execution. The shell preview implementation and composed invoice modal remain owed by p1V, pending generated consumer outputs. The updated fixture facade is not generated fixture output.

**X source closure / generator GO:** `implementation/core.md` sections 3–4 now publish the final exporter and composed contract; R/X/p23 source-publication blockers are closed. X sent generator GO to Y and holds owned source for capture. The preview guard is `AppState::is_current_invoice_preview(ticket, payment_id)` immediately before delivery, checking signed-in billing route, exact ticket and R's `is_previewing(payment_id)`; close/reopen of the same payment invalidates old delivery and failure. `ConfirmMessages.status` remains optional and `BillingMessages.document_failure` is included. Actual generated outputs, p1V rendering integration, and Y diagnostics are still pending; no execution claim is inferred from GO.

### Run5 rendering source handoff (2026-10-26)

- Purchase: desktop plans/summary and phone selection-expanded bottom sheet; forwards typed purchase intents, keeps hour draft text only as a UI mechanic, displays `display_total` rather than recomputing from `charged_quote`, uses server-composed renewal timestamp/copy, and blocks dismissal/second payment while processing. Terms copy is escaped text with only the catalog's known anchor rendered as a fixed legal link; no HTML injection.
- Subscription: mounts the settings tab; shows core-provided plan/instants, upgrade action, cancellation confirmation/in-flight lock, failure, and canceled/keep-minutes state. No renewal arithmetic.
- Invoices: desktop table and phone cards, owner/admin distinction from `admin`, customer lookup/clear, temporary Bill To and preview/download intents. All financial values are server strings. PDF bytes are only displayed in a named iframe inside the invoice dialog; no preview-to-download/new-window fallback.
- `InvoicePreview.sync(AppVm)` releases URLs synchronously on selection/account/customer/Bill-To/route changes. Replacement revokes the old URL; close/reopen starts empty; disposal releases the current URL. Allocation failures throw to the existing Rust port boundary. Same-payment request-ticket freshness remains p22/X-owned and is not reimplemented in TypeScript.
- Confirmation displays optional `status` only when present and renders the unconfirmed warning once. Countdown/redirect/analytics remain core/driver effects. Special-offer message and code remain literal host data; dismissal is local to the signed-in frame.
- Small primitive changes: `Dialog.wide?` for desktop purchase/PDF; restored the missing `Menu.Props.trigger?: Snippet` reported by p1Z, with no change to label/items-only callers.
- New regression source covers shown-versus-charged amount, blocked payment/dismissal, coupon Enter versus checkout submission, phone section visibility, canceled/in-flight subscriptions, admin isolation, inline preview/document errors, confirmation warning/countdown, escaped offer data, and URL lifetime/failure. No tests executed by p1V.
- No dependency/config/font/token changes. No Rust/client/wasm/locale edits. Existing checkout templates and frozen inputs untouched. Generic story-root existence test removed rather than treated as rendering proof; real C1/C2 proof remains Y-owned.

Review limits: generated card strings (`PaygCardView.price_text`, plan name/badge) are consumed as published; no missing plan entitlements or locale policy invented in the shell. This handoff establishes source readiness only. Actual visual parity, native PDF iframe behavior, all-locale rendering, TypeScript/Svelte diagnostics and wasm-driven flows are still unverified by this owner.

### REPAIR-P5-1 source handoff (2026-10-26)

This section supersedes the earlier source-only verification status for the checks explicitly listed here. Source checkout only; candidate3 was not edited. No browser, Storybook, provider, Rust build, fixture generation, lockfile change or formatter run by p1V.

- **Duplicate desktop navigation:** `features/app/views/desktop/DesktopSidebar.svelte` no longer repeats expanded New Transcript, All Transcripts or Recent transcripts chrome supplied by `SidebarHistory`. Collapsed frame actions remain. `features/app/containers/SignedInFrame.test.ts` covers the real composition and collapsed navigation; `features/app/views/frame.test.ts` now reflects slot ownership.
- **Keyboard scrolling:** `ui/Dialog.svelte` moves overflow into an explicitly named `role="region"` with `tabindex="0"`; the outer native dialog clips rather than scrolling. This retains keyboard access when processing disables all purchase controls. `features/commerce/views/mobile/MobilePurchase.test.ts` covers focusability during processing. Lead explicitly approved the one-element `svelte-ignore a11y_no_noninteractive_tabindex`, documented immediately above it with WCAG 2.1.1 / axe `scrollable-region-focusable`; no file-wide/config suppression. Actual axe closure remains Y-owned.
- **Root landscape coverage:** `features/app/containers/App.svelte` explicitly marks the mounted application and cookie banner inert during the landscape notice, keeping the notice outside that subtree. `App.test.ts` covers mounted auth content, prevented Escape dismissal, portrait recovery and retained input identity. jsdom lacks inert-attribute reflection, so the test observes the actual `inert` DOM property; this does not claim native focus-trap proof.
- **Root story:** new `features/app/containers/AppStory.svelte` hosts the real App with a fixed generated fixture and no wasm/provider effects. `App.stories.ts` adds `Pages/App/Root/PhoneLandscape` using `fixture("AppVm", "phone_job")`, with the app beneath the notice. Screen orientation/coarse-pointer signals are explicitly simulated because resizing Storybook's iframe does not rotate the physical screen; cleanup restores them. Its play function checks the inert mounted phone frame and Escape lock. Source type-checked, not run in Storybook by this owner.
- **Owned diagnostics:** corrected `features/commerce/views/PurchaseDialog.svelte` draft initialization and keyboard-event typing; `features/auth/views/Turnstile.svelte` non-null promise return; `ui/SplitPane.svelte` component instance typing; `ui/TextField.stories.ts` directly typed partial component args instead of StoryObj's excessive native-attribute union expansion; `i18n/index.svelte.ts` checked false-literal options; `features/app/containers/App.svelte` intentionally untracked initial presenter snapshot; `platform/browser.ts` browser constructor types; and `platform/telemetry.test.ts` deferred promise compatible with the declared target. No `any` casts or compiler-option weakening.
- **Test defects exposed by execution:** `platform/preview.test.ts` clears a prequeued mock success before injecting URL allocation failure; production exception behavior is unchanged. `features/auth/views/AuthView.test.ts` now proves native constraints reject an incomplete draft, then supplies valid email/password before expecting submission.
- **Peer seam:** p1Z retains player/history/transcript ownership, including local `$state`-shadowing fixes. `SidebarHistory` props remain unchanged. No p1Z files edited.

Verification by p1V:

```sh
npm run test:ci -- src/lib/features/app src/lib/features/commerce src/lib/features/auth src/lib/features/feedback src/lib/features/support src/lib/i18n src/lib/ui/primitives.test.ts src/lib/platform/browser.test.ts src/lib/platform/telemetry.test.ts src/lib/platform/preview.test.ts src/lib/platform/landscape.test.ts src/lib/platform/viewport.test.ts
npm run build
```

- New sidebar and scroll regressions first failed at 19:45:46; root landscape coverage first failed at 19:47:31. Final scoped Vitest run at 19:51:10: **exit 0, 23 files / 85 tests passed**. Intermediate auth/preview test defects and jsdom inert-reflection mismatch were repaired, not hidden with production fallbacks.
- Serialized `npm run build`: **exit 1; Svelte-check 1412 files, 1 error, 0 warnings**. Sole remaining error: `src/lib/core/index.ts:5:42`, missing generated `./pkg/transgate_wasm.js` or declarations. All p1V diagnostics cleared. Vite compilation did not run; an application build pass is not claimed. Y/lead owns the generated wasm prerequisite and subsequent runtime/C1/C2 verification.
- Dependency collision evidence retained: overlapping earlier installs left Vitest unavailable. Lead-authorized sole-owner corrective `npm ci --no-audit --no-fund` restored the committed-lock install; `npx --no-install vitest --version` returned `vitest/5.0.5` on Node 26.2.0, followed by actual scoped test execution above. p1Z/p21 received fresh readiness; p1V remains the only authorized source-directory installer. This local run is not the pinned Node24 integration gate.

Lead disposition: the missing package is generated wasm-bindgen glue, now gitignored. Y will publish it once in the source checkout via `scripts/build-transgate-wasm.sh`, then notify p1V/p1Z. Until publication, the missing-pkg diagnostic is expected; no stub is permitted.

**Final build update:** Y reported successful source wasm-glue publication (`TASKR_WASM_BUILD_STD=1 ./scripts/build-transgate-wasm.sh`, receipt `/home/mu/.cache/rustr-integration/repair-p5-1-source-wasm.json`). After that release, p1V ran `npm run build` again: **exit 0; Svelte-check 1414 files, 0 errors, 0 warnings; Vite 954 modules transformed, built in 2.26s**. This clears the missing-pkg prerequisite and completes the authorized source build check. Browser/axe/Storybook and immutable-candidate certification remain unclaimed and Y-owned.

### REPAIR-P5-4: latest-request locale fence (2026-10-26)

Independent of candidate4's all-English start-URL defect, which remains p22-owned. Changed only `src/lib/i18n/index.svelte.ts`, `src/lib/i18n/index.test.ts`, new `src/lib/i18n/race.test.ts`, `src/lib/features/app/containers/App.svelte`, and this handoff.

- `Translator.use()` assigns each valid request a monotonically increasing identity. It checks ownership after initialization, after catalog loading and after language activation, before publishing locale/version. Superseded loads can populate their locale-keyed cache but cannot activate or publish an older locale. The public `Promise<void>` contract and error propagation remain unchanged.
- App's locale effect invalidates its completion callback on cleanup. An earlier effect cannot open the render gate after a newer locale request, including a return to the same locale.
- Failing-first regression delays real Turkish/German common-catalog imports through the production glob. German completes first; Turkish completes last. Before the fix, final locale was `tr` instead of `de` (20:28:14, exit 1). Afterward, every observed locale publication is exactly `de`, and the rendered-label lookup stays `Anmelden`: no intermediate stale publication.
- Sixteen parameterized cases create fresh translators, load all five real namespaces via the production glob without injected catalogs, and check `auth/sign_in.button_label`. English is `Sign in`; every non-English locale differs from English and from the untranslated key.

Verification:

- `npm run test:ci -- src/lib/i18n src/lib/features/app/containers/App.test.ts`: **exit 0, 3 files / 24 tests passed**, 20:29:57.
- Serialized `npm run build`: **exit 0, 1415 files / 0 errors / 0 warnings**; Vite transformed 954 modules and built in 2.40s.
- Throwaway Node/Vite module smoke loaded the actual translator via `ssrLoadModule`, requested Turkish then German concurrently and observed `{"locale":"de","label":"Anmelden"}`, exit 0. Middleware-mode Vite was closed in `finally`; no persistent server or script created. This is module execution, not browser visual proof.

No installs, peer source edits, generated-source changes, provider requests, candidate changes or browser certification. Build slot released to p1Z; source-ready handoff goes to lead w23:p2B. The App cleanup fence is covered by compilation and existing root regression, not a dedicated out-of-order root-render test; the deterministic out-of-order test directly exercises the translator and its publications.
