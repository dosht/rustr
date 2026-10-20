// Every story renders without errors, so a broken story fails `npm run test:ci`
// instead of being found by someone opening Storybook.
import { composeStories } from "@storybook/svelte";
import { cleanup, render } from "@testing-library/svelte";
import type { Component } from "svelte";
import { describe, expect, it } from "vitest";

type StoriesModule = Parameters<typeof composeStories>[0];
type Composed = { Component: Component<Record<string, unknown>>; props: Record<string, unknown> };

const modules = import.meta.glob<StoriesModule>("./**/*.stories.ts", { eager: true });

describe.each(Object.entries(modules))("%s", (_path, module) => {
  const stories = Object.entries(composeStories(module)) as [string, Composed][];
  it.each(stories)("%s renders", (_name, story) => {
    const { container } = render(story.Component, story.props);
    expect(container.textContent?.trim().length).toBeGreaterThan(0);
    cleanup();
  });
});
