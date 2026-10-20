/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** API root, e.g. /api (proxied in dev) or https://app.example.com/api */
  readonly VITE_TASKR_API?: string;
  /** Supabase Auth root, e.g. https://<project>.supabase.co/auth/v1 or /auth (proxied) */
  readonly VITE_TASKR_AUTH_URL?: string;
  /** Supabase anon (publishable) key */
  readonly VITE_TASKR_AUTH_KEY?: string;
}
