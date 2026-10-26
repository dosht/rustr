import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import MobileInvoices from "./MobileInvoices.svelte";

const meta = {
  title: "Features/Commerce/MobileViews/Invoices",
  component: MobileInvoices,
  parameters: { viewport: { defaultViewport: "phone" } },
  args: { onintent: fn(), onback: fn(), previewUrl: null },
} satisfies Meta<typeof MobileInvoices>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Loading: Story = { args: { vm: fixture("InvoicesVm", "loading"), messages: fixture("BillingMessages", "invoices_loading") } };
export const Owner: Story = { args: { vm: fixture("InvoicesVm", "owner"), messages: fixture("BillingMessages", "invoices_owner") } };
export const Empty: Story = { args: { vm: fixture("InvoicesVm", "empty"), messages: fixture("BillingMessages", "invoices_empty") } };
export const Failed: Story = { args: { vm: fixture("InvoicesVm", "failed"), messages: fixture("BillingMessages", "invoices_failed") } };
export const Preview: Story = { args: { vm: fixture("InvoicesVm", "preview"), messages: fixture("BillingMessages", "invoices_preview") } };
export const DocumentFailed: Story = { args: { vm: fixture("InvoicesVm", "document_failed"), messages: fixture("BillingMessages", "invoices_document_failed") } };
export const Admin: Story = { args: { vm: fixture("InvoicesVm", "admin"), messages: fixture("BillingMessages", "invoices_admin") } };
export const AdminLookingUp: Story = { args: { vm: fixture("InvoicesVm", "admin_looking_up"), messages: fixture("BillingMessages", "invoices_admin_looking_up") } };
export const AdminCustomer: Story = { args: { vm: fixture("InvoicesVm", "admin_customer"), messages: fixture("BillingMessages", "invoices_admin_customer") } };
export const AdminNotFound: Story = { args: { vm: fixture("InvoicesVm", "admin_not_found"), messages: fixture("BillingMessages", "invoices_admin_not_found") } };
