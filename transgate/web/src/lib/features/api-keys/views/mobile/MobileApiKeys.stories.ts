import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import MobileApiKeys from "./MobileApiKeys.svelte";

const meta = {
  title: "Features/ApiKeys/MobileViews/ApiKeys",
  component: MobileApiKeys,
  parameters: { viewport: { defaultViewport: "phone" } },
  args: { onintent: fn() },
} satisfies Meta<typeof MobileApiKeys>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Loading: Story = { args: { vm: fixture("ApiKeysVm", "loading") } };
export const Empty: Story = { args: { vm: fixture("ApiKeysVm", "empty") } };
export const List: Story = { args: { vm: fixture("ApiKeysVm", "list") } };
export const CreatingForm: Story = { args: { vm: fixture("ApiKeysVm", "creating_form") } };
export const Created: Story = { args: { vm: fixture("ApiKeysVm", "created") } };
export const ConfirmRevoke: Story = { args: { vm: fixture("ApiKeysVm", "confirm_revoke") } };
export const LoadFailed: Story = { args: { vm: fixture("ApiKeysVm", "load_failed") } };
