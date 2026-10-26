import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import DesktopPurchase from "./DesktopPurchase.svelte";

const meta = {
  title: "Features/Commerce/DesktopViews/Purchase",
  component: DesktopPurchase,
  args: { onintent: fn(), onclose: fn(), ondismissoffer: fn(), oncopy: fn().mockResolvedValue(true) },
} satisfies Meta<typeof DesktopPurchase>;
export default meta;
type Story = StoryObj<typeof meta>;

export const QuotePending: Story = { args: { vm: fixture("PurchaseVm", "quote_pending"), messages: fixture("BillingMessages", "purchase_quote_pending") } };
export const PayAsYouGo: Story = { args: { vm: fixture("PurchaseVm", "pay_as_you_go"), messages: fixture("BillingMessages", "purchase_pay_as_you_go") } };
export const DisplayDiffersFromCharge: Story = { args: { vm: fixture("PurchaseVm", "display_differs_from_charge"), messages: fixture("BillingMessages", "purchase_display_differs_from_charge") } };
export const TurkishLira: Story = { args: { vm: fixture("PurchaseVm", "turkish_lira"), messages: fixture("BillingMessages", "purchase_turkish_lira") } };
export const YearlyPlan: Story = { args: { vm: fixture("PurchaseVm", "yearly_plan"), messages: fixture("BillingMessages", "purchase_yearly_plan") } };
export const CouponExpired: Story = { args: { vm: fixture("PurchaseVm", "coupon_expired"), messages: fixture("BillingMessages", "purchase_coupon_expired") } };
export const ReadyToPay: Story = { args: { vm: fixture("PurchaseVm", "ready_to_pay"), messages: fixture("BillingMessages", "purchase_ready_to_pay") } };
export const Processing: Story = { args: { vm: fixture("PurchaseVm", "processing"), messages: fixture("BillingMessages", "purchase_processing") } };
export const Failed: Story = { args: { vm: fixture("PurchaseVm", "failed"), messages: fixture("BillingMessages", "purchase_failed") } };
