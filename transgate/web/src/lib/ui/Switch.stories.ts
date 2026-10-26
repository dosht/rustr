import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import Switch from "./Switch.svelte";

const meta = {
  title: "UI/Primitives/Switch",
  component: Switch,
  args: { checked: true, label: "Automatic transcript cleanup", description: "Transcripts older than 30 days are deleted.", onchange: fn() },
} satisfies Meta<typeof Switch>;

export default meta;
type Story = StoryObj<typeof meta>;

export const On: Story = { args: {} };
export const Off: Story = { args: { checked: false } };
export const Disabled: Story = { args: { disabled: true } };
