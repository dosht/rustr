import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import fixtures from "$lib/core/fixtures.json";
import type { SupportVm } from "$lib/core/types/SupportVm";
import MobileSupport from "./MobileSupport.svelte";

const vms = fixtures.SupportVm as Record<string, SupportVm>;

const meta = {
  title: "Features/Support/MobileViews/Support",
  component: MobileSupport,
  args: { onmessage: fn(), onsubmit: fn(), onclose: fn() },
  parameters: { viewport: { defaultViewport: "phone" } },
} satisfies Meta<typeof MobileSupport>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Typing: Story = { args: { vm: vms.typing } };
export const Sending: Story = { args: { vm: vms.sending } };
export const Failed: Story = { args: { vm: vms.failed } };
