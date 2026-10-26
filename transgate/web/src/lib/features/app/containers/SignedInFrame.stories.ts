import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import { createMediaPlatform } from "$lib/platform/media";
import { ThemeStore } from "$lib/theme/theme.svelte";
import SignedInFrame from "./SignedInFrame.svelte";

const meta = {
  title: "Pages/App/SignedInFrame",
  component: SignedInFrame,
  args: {
    dispatch: fn(), media: createMediaPlatform(), theme: new ThemeStore(),
    previewUrl: null, oncopy: fn().mockResolvedValue(true),
  },
} satisfies Meta<typeof SignedInFrame>;
export default meta;
type Story = StoryObj<typeof meta>;
export const DesktopNew: Story = { args: { vm: fixture("SignedInVm", "desktop_new") } };
export const PhoneJob: Story = { args: { vm: fixture("SignedInVm", "phone_job") } };
export const SettingsOpen: Story = { args: { vm: fixture("SignedInVm", "settings_open") } };
export const PurchaseOpen: Story = { args: { vm: fixture("SignedInVm", "purchase_open") } };
export const BillingOwner: Story = { args: { vm: fixture("SignedInVm", "billing_owner") } };
export const SpecialOffer: Story = { args: { vm: fixture("SignedInVm", "special_offer") } };
