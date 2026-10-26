<script lang="ts" module>
  // Cloudflare Turnstile, rendered explicitly (source signin.tsx:174 used
  // @marsidev/react-turnstile). A shell mechanic: the token goes to core as an
  // intent and core forwards it to GoTrue (D12). The script loads only when a
  // site key is configured and this widget mounts; CSP allows the origin then.
  interface TurnstileApi {
    render(element: HTMLElement, options: Record<string, unknown>): string;
    remove(widget: string): void;
  }

  const SCRIPT = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
  let loading: Promise<TurnstileApi> | null = null;

  function load(): Promise<TurnstileApi> {
    if (loading) return loading;
    const api = () => (window as Window & { turnstile?: TurnstileApi }).turnstile;
    const existing = api();
    if (existing) return (loading = Promise.resolve(existing));
    const script = document.createElement("script");
    script.src = SCRIPT;
    script.async = true;
    const promise = new Promise<TurnstileApi>((resolve, reject) => {
      script.onload = () => {
        const loaded = api();
        if (loaded) resolve(loaded);
        else reject(new Error("Turnstile did not initialize"));
      };
      script.onerror = () => reject(new Error("Turnstile failed to load"));
      document.head.append(script);
    });
    loading = promise.catch((error: unknown) => {
      loading = null;
      script.remove();
      throw error;
    });
    return loading;
  }
</script>

<script lang="ts">
  interface Props {
    siteKey: string;
    /** UI locale, so the challenge speaks the page's language. */
    language: string;
    onsolved: (token: string) => void;
    onexpired: () => void;
  }

  let { siteKey, language, onsolved, onexpired }: Props = $props();
  let element: HTMLDivElement | undefined = $state();

  $effect(() => {
    if (!element) return;
    let widget: string | null = null;
    let api: TurnstileApi | null = null;
    let disposed = false;
    load()
      .then((loaded) => {
        if (disposed || !element) return;
        api = loaded;
        widget = loaded.render(element, {
          sitekey: siteKey,
          language,
          callback: onsolved,
          "expired-callback": onexpired,
          "error-callback": onexpired,
        });
      })
      .catch((error: unknown) => {
        // As in the source, the form stays unsubmittable without a token.
        if (!disposed) console.error(error);
      });
    return () => {
      disposed = true;
      if (api && widget !== null) api.remove(widget);
    };
  });
</script>

<div bind:this={element} class="flex justify-center" data-testid="turnstile"></div>
