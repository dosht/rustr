import type { StorybookConfig } from "@storybook/svelte-vite";

const config: StorybookConfig = {
  framework: "@storybook/svelte-vite",
  stories: ["../src/**/*.stories.ts"],
  addons: ["@storybook/addon-docs", "@storybook/addon-a11y"],
};

export default config;
