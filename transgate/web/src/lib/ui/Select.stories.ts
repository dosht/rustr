import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import Select from "./Select.svelte";

const meta = {
  title: "UI/Primitives/Select",
  component: Select,
  args: { label: "Language", value: "en", options: [{ value: "en", label: "English" }, { value: "tr", label: "Türkçe" }], onchange: fn() },
} satisfies Meta<typeof Select>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: {} };
export const HiddenLabel: Story = { args: { hideLabel: true } };
