import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { membersScreen } from "../members.fixtures";
import MembersDesktop from "./MembersDesktop.svelte";

const meta = {
  title: "Features/Members/DesktopViews/Members",
  component: MembersDesktop,
  args: { onIntent: fn(), heading: "Members" },
} satisfies Meta<typeof MembersDesktop>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Owner: Story = { args: { view: membersScreen("owner-members") } };
export const Viewer: Story = { args: { view: membersScreen("viewer-members") } };
