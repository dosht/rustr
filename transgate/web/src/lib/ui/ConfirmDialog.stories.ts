import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import ConfirmDialog from "./ConfirmDialog.svelte";

const meta = {
  title: "UI/Primitives/ConfirmDialog",
  component: ConfirmDialog,
  args: { open: true, title: "Delete account?", description: "This action cannot be undone.", confirmLabel: "Delete", cancelLabel: "Cancel", tone: "danger", onconfirm: fn(), oncancel: fn() },
} satisfies Meta<typeof ConfirmDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Danger: Story = { args: {} };
export const Busy: Story = { args: { busy: true } };
export const Sheet: Story = { args: { kind: "sheet" } };
