import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { taskScreen } from "../../../core/fixtures";
import TaskListDesktop from "./TaskListDesktop.svelte";

// Every story is a fixture the Rust core produced (`fixtures.json`), so what
// you see here is exactly what the core would hand this view.
const meta = {
  title: "Features/Tasks/DesktopViews/TaskList",
  component: TaskListDesktop,
  args: { onIntent: fn(), heading: "ada's workspace" },
} satisfies Meta<typeof TaskListDesktop>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Sample: Story = { args: { view: taskScreen("sample") } };
export const Empty: Story = { args: { view: taskScreen("empty") } };
export const Loading: Story = { args: { view: taskScreen("loading") } };
export const DuplicateError: Story = { args: { view: taskScreen("error") } };
export const ReadOnly: Story = { args: { view: taskScreen("read-only") } };
