/// <reference types="vite/client" />

// `vite dev` only: stand-ins for the host-injected tg-config (web-shell.md section 7).
interface ImportMetaEnv {
  readonly VITE_TG_API_URL?: string;
  readonly VITE_TG_AUTH_URL?: string;
  readonly VITE_TG_AUTH_KEY?: string;
  readonly VITE_TG_SITE_URL?: string;
  readonly VITE_TG_TURNSTILE_SITE_KEY?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
