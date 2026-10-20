import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { signedIn } from "../../../core/fixtures";
import DesktopFrame from "./DesktopFrame.svelte";

const meta = {
  title: "Pages/App/DesktopViews/Frame",
  component: DesktopFrame,
  args: { onIntent: fn() },
} satisfies Meta<typeof DesktopFrame>;

export default meta;
type Story = StoryObj<typeof meta>;

export const OwnerTasks: Story = { args: { view: signedIn("owner-tasks") } };
export const ViewerTasks: Story = { args: { view: signedIn("viewer-tasks") } };
export const OwnerMembers: Story = { args: { view: signedIn("owner-members") } };
export const ViewerMembers: Story = { args: { view: signedIn("viewer-members") } };
export const Loading: Story = { args: { view: signedIn("loading") } };
