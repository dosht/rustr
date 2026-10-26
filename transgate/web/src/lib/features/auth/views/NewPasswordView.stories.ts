import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import fixtures from "$lib/core/fixtures.json";
import type { NewPasswordVm } from "$lib/core/types/NewPasswordVm";
import NewPasswordView from "./NewPasswordView.svelte";

const vms = fixtures.NewPasswordVm as Record<string, NewPasswordVm>;

const meta = {
  title: "Pages/App/NewPassword",
  component: NewPasswordView,
  args: { onintent: fn() },
} satisfies Meta<typeof NewPasswordView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = { args: { vm: vms.empty } };
export const Busy: Story = { args: { vm: vms.busy } };
export const Failed: Story = { args: { vm: vms.error } };
export const Success: Story = { args: { vm: vms.success } };
