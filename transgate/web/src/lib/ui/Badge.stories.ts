import type { Meta, StoryObj } from "@storybook/svelte";
import { createRawSnippet } from "svelte";
import Badge from "./Badge.svelte";

const text = (value: string) => createRawSnippet(() => ({ render: () => `<span>${value}</span>` }));

const meta = {
  title: "UI/Primitives/Badge",
  component: Badge,
  args: { tone: "progress", spinner: true, children: text("RUNNING") },
} satisfies Meta<typeof Badge>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Progress: Story = { args: {} };
export const Success: Story = { args: { tone: "success", spinner: false, children: text("COMPLETED") } };
export const Danger: Story = { args: { tone: "danger", spinner: false, children: text("FAILED") } };
export const Neutral: Story = { args: { tone: "neutral", spinner: false, children: text("DRAFT") } };
