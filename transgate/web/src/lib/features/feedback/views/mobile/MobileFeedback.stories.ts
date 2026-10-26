import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import fixtures from "$lib/core/fixtures.json";
import type { FeedbackVm } from "$lib/core/types/FeedbackVm";
import MobileFeedback from "./MobileFeedback.svelte";

const vms = fixtures.FeedbackVm as Record<string, FeedbackVm>;

const meta = {
  title: "Features/Feedback/MobileViews/Feedback",
  component: MobileFeedback,
  args: { onselect: fn(), oncomment: fn(), onsubmit: fn(), onclose: fn() },
  parameters: { viewport: { defaultViewport: "phone" } },
} satisfies Meta<typeof MobileFeedback>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = { args: { vm: vms.empty } };
export const SelectFirst: Story = { args: { vm: vms.select_first } };
export const Submitting: Story = { args: { vm: vms.submitting } };
export const Thanked: Story = { args: { vm: vms.thanked } };
export const Failed: Story = { args: { vm: vms.failed } };
