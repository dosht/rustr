import type { Meta, StoryObj } from "@storybook/svelte";
import Skeleton from "./Skeleton.svelte";

const meta = {
  title: "UI/Primitives/Skeleton",
  component: Skeleton,
  args: { label: "Loading...", lines: 3 },
} satisfies Meta<typeof Skeleton>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: {} };
