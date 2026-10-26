import type { Meta, StoryObj } from "@storybook/svelte";
import { createRawSnippet } from "svelte";
import { fn } from "storybook/test";
import Alert from "./Alert.svelte";

const text = (value: string) => createRawSnippet(() => ({ render: () => `<span>${value}</span>` }));

const meta = {
  title: "UI/Primitives/Alert",
  component: Alert,
  args: { tone: "error", title: "Something went wrong", children: text("Please try again."), ondismiss: fn(), dismissLabel: "Dismiss" },
} satisfies Meta<typeof Alert>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Error: Story = { args: {} };
export const Info: Story = { args: { tone: "info", title: undefined, children: text("Your files are being prepared.") } };
export const Success: Story = { args: { tone: "success", title: undefined, children: text("Saved.") } };
