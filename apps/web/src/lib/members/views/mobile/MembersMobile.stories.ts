import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { membersScreen } from "../members.fixtures";
import MembersMobile from "./MembersMobile.svelte";

const meta = {
  title: "Features/Members/MobileViews/Members",
  component: MembersMobile,
  args: { onIntent: fn() },
  globals: { viewport: { value: "mobile1", isRotated: false } },
} satisfies Meta<typeof MembersMobile>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Owner: Story = { args: { view: membersScreen("owner-members") } };
export const Viewer: Story = { args: { view: membersScreen("viewer-members") } };
