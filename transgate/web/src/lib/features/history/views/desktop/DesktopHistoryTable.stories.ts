import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import DesktopHistoryTable from "./DesktopHistoryTable.svelte";

const meta = {
  title: "Features/History/DesktopViews/HistoryTable",
  component: DesktopHistoryTable,
  args: {
    onopen: fn(),
    ondownload: fn(),
    onfeedback: fn(),
    ondelete: fn(),
    onconfirmdelete: fn(),
    oncanceldelete: fn(),
    onloadmore: fn(),
  },
} satisfies Meta<typeof DesktopHistoryTable>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Sample: Story = { args: { vm: fixture("JobListVm", "sample") } };
export const Empty: Story = { args: { vm: fixture("JobListVm", "empty") } };
export const Loading: Story = { args: { vm: fixture("JobListVm", "loading") } };
export const LoadingMore: Story = { args: { vm: fixture("JobListVm", "loading_more") } };
export const LoadError: Story = { args: { vm: fixture("JobListVm", "error") } };
export const ConfirmDelete: Story = { args: { vm: fixture("JobListVm", "confirm_delete") } };
export const Downloading: Story = { args: { vm: fixture("JobListVm", "downloading") } };
