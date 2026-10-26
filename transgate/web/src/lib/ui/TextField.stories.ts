import type { Meta } from "@storybook/svelte";
import type { ComponentProps } from "svelte";
import { fn } from "storybook/test";
import TextField from "./TextField.svelte";

const meta = {
  title: "UI/Primitives/TextField",
  component: TextField,
  args: { label: "Email", value: "", type: "email", oninput: fn() },
} satisfies Meta<typeof TextField>;

export default meta;
// Keep the component's discriminated input/textarea props without StoryObj's
// combinatorial mapping over every native HTML event attribute.
type Story = { args: Partial<ComponentProps<typeof TextField>> };

export const Empty: Story = { args: {} };
export const Invalid: Story = { args: { value: "ada@", error: "Enter a valid email" } };
export const Multiline: Story = { args: { label: "Message", multiline: true, rows: 4, value: "" } };
