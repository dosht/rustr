import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { signedIn } from "../../../core/fixtures";
import MobileFrame from "./MobileFrame.svelte";

const meta = {
  title: "Pages/App/MobileViews/Frame",
  component: MobileFrame,
  args: { onIntent: fn() },
  globals: { viewport: { value: "mobile1", isRotated: false } },
} satisfies Meta<typeof MobileFrame>;

export default meta;
type Story = StoryObj<typeof meta>;

export const OwnerTasks: Story = { args: { view: signedIn("owner-tasks") } };
export const ViewerTasks: Story = { args: { view: signedIn("viewer-tasks") } };
export const OwnerMembers: Story = { args: { view: signedIn("owner-members") } };
export const ViewerMembers: Story = { args: { view: signedIn("viewer-members") } };
