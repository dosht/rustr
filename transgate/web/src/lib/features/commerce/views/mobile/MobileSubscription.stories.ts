import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import MobileSubscription from "./MobileSubscription.svelte";

const meta = {
  title: "Features/Commerce/MobileViews/Subscription",
  component: MobileSubscription,
  parameters: { viewport: { defaultViewport: "phone" } },
  args: { onintent: fn() },
} satisfies Meta<typeof MobileSubscription>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Loading: Story = { args: { vm: fixture("SubscriptionVm", "loading"), messages: fixture("BillingMessages", "subscription_loading") } };
export const Active: Story = { args: { vm: fixture("SubscriptionVm", "active"), messages: fixture("BillingMessages", "subscription_active") } };
export const NoPlan: Story = { args: { vm: fixture("SubscriptionVm", "no_plan"), messages: fixture("BillingMessages", "subscription_no_plan") } };
export const Confirming: Story = { args: { vm: fixture("SubscriptionVm", "confirming"), messages: fixture("BillingMessages", "subscription_confirming") } };
export const Cancelling: Story = { args: { vm: fixture("SubscriptionVm", "cancelling"), messages: fixture("BillingMessages", "subscription_cancelling") } };
export const Canceled: Story = { args: { vm: fixture("SubscriptionVm", "canceled"), messages: fixture("BillingMessages", "subscription_canceled") } };
export const CancelFailed: Story = { args: { vm: fixture("SubscriptionVm", "cancel_failed"), messages: fixture("BillingMessages", "subscription_cancel_failed") } };
