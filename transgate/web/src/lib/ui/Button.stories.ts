import type { Meta, StoryObj } from "@storybook/svelte";
import { createRawSnippet } from "svelte";
import { fn } from "storybook/test";
import Button from "./Button.svelte";

const text = (value: string) => createRawSnippet(() => ({ render: () => `<span>${value}</span>` }));

const meta = {
  title: "UI/Primitives/Button",
  component: Button,
  args: { variant: "primary", children: text("Start transcribing"), onclick: fn() },
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = { args: {} };
export const Secondary: Story = { args: { variant: "secondary", children: text("Cancel") } };
export const Danger: Story = { args: { variant: "danger", children: text("Delete") } };
export const Busy: Story = { args: { busy: true, children: text("Submitting...") } };
export const Disabled: Story = { args: { disabled: true } };
