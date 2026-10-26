import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import MobileHistoryList from "./MobileHistoryList.svelte";

const meta = {
  title: "Features/History/MobileViews/HistoryList",
  component: MobileHistoryList,
  parameters: { viewport: { defaultViewport: "phone" } },
  args: {
    onopen: fn(),
    ondelete: fn(),
    onconfirmdelete: fn(),
    oncanceldelete: fn(),
    onloadmore: fn(),
    onrefresh: fn(),
    onnew: fn(),
  },
} satisfies Meta<typeof MobileHistoryList>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Sample: Story = { args: { vm: fixture("JobListVm", "sample") } };
export const Empty: Story = { args: { vm: fixture("JobListVm", "empty") } };
export const Loading: Story = { args: { vm: fixture("JobListVm", "loading") } };
export const LoadingMore: Story = { args: { vm: fixture("JobListVm", "loading_more") } };
export const LoadError: Story = { args: { vm: fixture("JobListVm", "error") } };
export const ConfirmDelete: Story = { args: { vm: fixture("JobListVm", "confirm_delete") } };
