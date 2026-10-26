// The one translation door for every view. Catalogs, options and namespaces
// come from p24's locales/config.json, so the runtime and the validator
// (scripts/p5-locales) cannot disagree. There is no runtime fallback language:
// a missing key is a bug the validator should have caught, and in dev and
// tests it throws instead of quietly showing another language.
import i18next, { type i18n as I18n } from "i18next";
import config from "../../../locales/config.json";

export const LOCALES: readonly string[] = config.locales;
export const DEFAULT_LOCALE: string = config.defaultLocale;
export const NAMESPACES: readonly string[] = config.namespaces;

/** Structural form of core's message (`{ns, key, args}`, config.message). */
export interface Message {
  ns: string;
  key: string;
  args?: Record<string, MessageArg> | null;
}
export type MessageArg = string | number | Message;

type Catalog = Record<string, string>;
type Loader = () => Promise<Catalog>;

const loaders = import.meta.glob<Catalog>("../../../locales/*/*.json", { import: "default" });

function loaderFor(locale: string, ns: string): Loader | undefined {
  return loaders[`../../../locales/${locale}/${ns}.json`];
}

function isMessage(value: MessageArg): value is Message {
  return typeof value === "object" && value !== null && "key" in value && "ns" in value;
}

export class Translator {
  #instance: I18n = i18next.createInstance();
  #ready: Promise<unknown>;
  #loaded = new Set<string>();
  #request = 0;
  /** Bumped when the language changes, so templates calling `t` re-render. */
  #version = $state(0);
  locale = $state(DEFAULT_LOCALE);

  readonly #strict: boolean;

  /** Strict by default; production renders the key and logs instead of throwing. */
  constructor(strict: boolean = import.meta.env.DEV || import.meta.env.MODE === "test") {
    this.#strict = strict;
    const options = config.i18next;
    if (options.fallbackLng !== false || options.keySeparator !== false || options.nsSeparator !== false) {
      throw new Error("UI catalogs require no fallback language and literal namespace/key lookup");
    }
    // Preserve the checked JSON policy, including false literals required by i18next.
    // init creates the resource store synchronously, so `add` works before `use`.
    this.#ready = this.#instance.init({
      ...options, fallbackLng: options.fallbackLng, keySeparator: options.keySeparator, nsSeparator: options.nsSeparator,
      lng: DEFAULT_LOCALE, ns: [...NAMESPACES], resources: {},
    });
  }

  async use(locale: string): Promise<void> {
    if (!LOCALES.includes(locale)) throw new Error(`unsupported UI locale: ${locale}`);
    const request = ++this.#request;
    await this.#ready;
    if (request !== this.#request) return;
    await this.#load(locale);
    // Stale loads may warm their cache, but must never activate an older locale.
    if (request !== this.#request) return;
    await this.#instance.changeLanguage(locale);
    if (request !== this.#request) return;
    this.locale = locale;
    this.#version += 1;
  }

  /** Adds catalogs directly, for tests and stories that must not touch the loader. */
  add(locale: string, ns: string, catalog: Catalog): void {
    this.#instance.addResourceBundle(locale, ns, catalog, true, true);
    this.#loaded.add(`${locale}/${ns}`);
  }

  async #load(locale: string): Promise<void> {
    for (const ns of NAMESPACES) {
      const id = `${locale}/${ns}`;
      if (this.#loaded.has(id)) continue;
      const load = loaderFor(locale, ns);
      if (!load) throw new Error(`missing catalog ${id}.json`);
      this.#instance.addResourceBundle(locale, ns, await load(), true, true);
      this.#loaded.add(id);
    }
  }

  /** Translates a core message; nested message arguments are translated first. */
  t(message: Message): string {
    return this.tk(message.ns, message.key, message.args ?? undefined);
  }

  /** Translates a view-owned static label (a key the view, not core, owns). */
  tk(ns: string, key: string, args?: Record<string, MessageArg>): string {
    void this.#version;
    const options: Record<string, unknown> = { ns, lng: this.locale };
    for (const [name, value] of Object.entries(args ?? {})) {
      options[name] = isMessage(value) ? this.t(value) : value;
    }
    if (!this.#instance.exists(key, options)) {
      const problem = `missing translation ${this.locale}/${ns}: ${JSON.stringify(key)}`;
      if (this.#strict) throw new Error(problem);
      console.error(problem);
      return key;
    }
    return this.#instance.t(key, options) as string;
  }
}

/** The app's translator; containers call `use(locale)` when the route locale changes. */
export const i18n = new Translator();
