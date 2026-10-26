// Light, dark or system, a shell-local mechanic (source ThemeProvider.tsx:
// key "theme", default "system"). `light`/`dark` set <html data-theme>;
// `system` removes it so the generated prefers-color-scheme rules apply.
// public/_app/theme-init.js applies the stored value before first paint; keep the
// two in step.

export type ThemeChoice = "light" | "dark" | "system";
export const THEME_KEY = "theme";
export const THEMES: readonly ThemeChoice[] = ["light", "dark", "system"];

function read(storage: Storage | undefined): ThemeChoice {
  try {
    const stored = storage?.getItem(THEME_KEY);
    return stored === "light" || stored === "dark" || stored === "system" ? stored : "system";
  } catch {
    return "system";
  }
}

export class ThemeStore {
  choice: ThemeChoice = $state("system");
  /** What `system` currently resolves to, for views that show it. */
  resolved: "light" | "dark" = $state("light");
  readonly #root: HTMLElement;
  readonly #storage: Storage | undefined;
  readonly #media: MediaQueryList | undefined;

  constructor(win: Window = window) {
    this.#root = win.document.documentElement;
    try {
      this.#storage = win.localStorage;
    } catch {
      this.#storage = undefined;
    }
    this.#media = win.matchMedia?.("(prefers-color-scheme: dark)");
    this.#media?.addEventListener("change", () => this.#apply());
    this.choice = read(this.#storage);
    this.#apply();
  }

  set(choice: ThemeChoice): void {
    this.choice = choice;
    try {
      this.#storage?.setItem(THEME_KEY, choice);
    } catch {
      // Storage blocked: the choice still applies for this page.
    }
    this.#apply();
  }

  #apply(): void {
    const systemDark = this.#media?.matches ?? false;
    this.resolved = this.choice === "system" ? (systemDark ? "dark" : "light") : this.choice;
    if (this.choice === "system") delete this.#root.dataset.theme;
    else this.#root.dataset.theme = this.choice;
  }
}
