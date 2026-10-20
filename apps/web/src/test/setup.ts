import "@testing-library/jest-dom/vitest";

// jsdom has no matchMedia; views never need it, the viewport store does.
if (!window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      addEventListener() {},
      removeEventListener() {},
    }) as unknown as MediaQueryList;
}
