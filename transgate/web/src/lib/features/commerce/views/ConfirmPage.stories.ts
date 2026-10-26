import type { Meta, StoryObj } from "@storybook/svelte";
import { fixture } from "$lib/core/fixtures";
import ConfirmPage from "./ConfirmPage.svelte";

const meta = { title: "Pages/App/CheckoutConfirmation", component: ConfirmPage } satisfies Meta<typeof ConfirmPage>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Loading: Story = { args: { messages: null } };
export const Succeeded: Story = { args: { messages: fixture("ConfirmMessages", "succeeded") } };
export const CountingDown: Story = { args: { messages: fixture("ConfirmMessages", "counting_down") } };
export const Declined: Story = { args: { messages: fixture("ConfirmMessages", "declined") } };
export const Pending: Story = { args: { messages: fixture("ConfirmMessages", "pending") } };
export const Unconfirmed: Story = { args: { messages: fixture("ConfirmMessages", "unconfirmed") } };
export const NoConfirmation: Story = { args: { messages: fixture("ConfirmMessages", "no_confirmation") } };
