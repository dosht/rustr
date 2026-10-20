import type { Preview } from "@storybook/svelte";
import "../src/app.css";

const preview: Preview = {
  parameters: {
    layout: "fullscreen",
    a11y: { test: "error" },
  },
};

export default preview;
