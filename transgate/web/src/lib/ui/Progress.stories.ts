import type { Meta, StoryObj } from "@storybook/svelte";
import Progress from "./Progress.svelte";

const meta = {
  title: "UI/Primitives/Progress",
  component: Progress,
  args: { label: "Uploading lecture.mp3", value: 42, showValue: true },
} satisfies Meta<typeof Progress>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Known: Story = { args: {} };
export const Indeterminate: Story = { args: { value: null } };
