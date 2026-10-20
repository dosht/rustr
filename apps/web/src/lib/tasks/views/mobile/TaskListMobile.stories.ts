import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { taskScreen } from "../../../core/fixtures";
import TaskListMobile from "./TaskListMobile.svelte";

const meta = {
  title: "Features/Tasks/MobileViews/TaskList",
  component: TaskListMobile,
  args: { onIntent: fn(), heading: "taskr" },
  globals: { viewport: { value: "mobile1", isRotated: false } },
} satisfies Meta<typeof TaskListMobile>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Sample: Story = { args: { view: taskScreen("sample") } };
export const Empty: Story = { args: { view: taskScreen("empty") } };
export const DuplicateError: Story = { args: { view: taskScreen("error") } };
export const ReadOnly: Story = { args: { view: taskScreen("read-only") } };
