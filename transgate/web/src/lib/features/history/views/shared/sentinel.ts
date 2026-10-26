// Infinite scroll as in the source (`useInfiniteScroll`, `MobileJobListView` 95% rule): when the
// sentinel at the end of the list becomes visible, ask core for the next page. Core decides whether
// a page is due (`can_load_more`); this only reports visibility.

export interface SentinelOptions {
  /** Whether a further page may be requested now. */
  enabled: boolean;
  onvisible: () => void;
}

export function sentinel(node: HTMLElement, initial: SentinelOptions) {
  let options = initial;
  // Environments without IntersectionObserver (jsdom) simply never page on scroll.
  if (typeof IntersectionObserver === "undefined") {
    return {
      update(next: SentinelOptions) {
        options = next;
      },
    };
  }
  const observer = new IntersectionObserver((entries) => {
    if (options.enabled && entries.some((entry) => entry.isIntersecting)) options.onvisible();
  });
  observer.observe(node);
  return {
    update(next: SentinelOptions) {
      options = next;
    },
    destroy() {
      observer.disconnect();
    },
  };
}
