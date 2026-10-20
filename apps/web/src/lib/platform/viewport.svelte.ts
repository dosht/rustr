// Whether to show the mobile or the desktop views. A shell mechanic, like
// focus or scrolling: the core never knows the screen size.
export const MOBILE_QUERY = "(max-width: 720px)";

export function createViewport(query = MOBILE_QUERY) {
  const media = typeof window !== "undefined" ? window.matchMedia?.(query) : undefined;
  let mobile = $state(media?.matches ?? false);
  media?.addEventListener("change", (e) => (mobile = e.matches));
  return {
    get mobile() {
      return mobile;
    },
  };
}
